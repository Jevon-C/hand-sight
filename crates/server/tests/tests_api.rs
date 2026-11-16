//! End-to-end tests of `GET /api/tests` and the real plug-ins shipped in `plugins/`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;

use axonhand_api::{ApiError, ErrorCode, TestDetail, TestList, TestStatus};
use axonhand_server::app::{build_jobs, router, Services};
use axonhand_server::probe::{ProbeFailure, Prober};
use axonhand_server::protocol::{complete_parameters, JobFile};
use axonhand_server::registry::Registry;
use axonhand_server::runner::{run, Limits};
use axonhand_server::store::Store;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn plugins_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins")
}

fn app_with(
    registry: Registry,
) -> Result<(axum::Router, tempfile::TempDir), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    std::fs::write(dir.path().join("index.html"), "<html></html>")?;
    let prober: Prober = Arc::new(|_| Err(ProbeFailure::Broken("unused".into())));
    let store = Arc::new(Store::open_in_memory()?);
    let registry_arc = Arc::new(registry);
    let jobs = build_jobs(
        &store,
        &registry_arc,
        std::path::Path::new("python"),
        &dir.path().join("videos"),
        dir.path(),
    )?;
    let services = Services {
        store,
        prober,
        video_dir: dir.path().join("videos"),
        temp_dir: dir.path().join("tmp"),
        max_upload_bytes: 1000,
        registry: registry_arc,
        jobs,
    };
    Ok((router(dir.path(), services), dir))
}

async fn get(
    app: &axum::Router,
    uri: &str,
) -> Result<(StatusCode, Vec<u8>), Box<dyn std::error::Error>> {
    let response = app
        .clone()
        .oneshot(Request::builder().uri(uri).body(Body::empty())?)
        .await?;
    let status = response.status();
    Ok((
        status,
        response.into_body().collect().await?.to_bytes().to_vec(),
    ))
}

#[tokio::test]
async fn the_shipped_plugins_are_listed_and_all_valid() -> TestResult {
    let (app, _dir) = app_with(Registry::scan(&plugins_dir()))?;
    let (status, body) = get(&app, "/api/tests").await?;
    assert_eq!(status, StatusCode::OK);
    let list: TestList = serde_json::from_slice(&body)?;
    assert!(
        list.tests.iter().all(|t| t.status != TestStatus::Broken),
        "a shipped plug-in is broken: {:?}",
        list.tests
            .iter()
            .filter(|t| t.status == TestStatus::Broken)
            .collect::<Vec<_>>()
    );
    let available: Vec<&str> = list
        .tests
        .iter()
        .filter(|t| t.status == TestStatus::Available)
        .map(|t| t.id.as_str())
        .collect();
    assert!(available.contains(&"example_fixed_values"));
    assert!(available.contains(&"finger_tapping"));
    for id in ["hand_open_close", "pronation_supination", "postural_tremor"] {
        assert!(available.contains(&id), "{id} should be available");
    }
    Ok(())
}

#[tokio::test]
async fn one_test_is_described_in_full() -> TestResult {
    let (app, _dir) = app_with(Registry::scan(&plugins_dir()))?;
    let (status, body) = get(&app, "/api/tests/example_fixed_values").await?;
    assert_eq!(status, StatusCode::OK);
    let detail: TestDetail = serde_json::from_slice(&body)?;
    assert_eq!(detail.metrics.len(), 2);
    assert_eq!(detail.parameters[0].id, "multiplier");
    Ok(())
}

#[tokio::test]
async fn an_unknown_test_is_not_found() -> TestResult {
    let (app, _dir) = app_with(Registry::default())?;
    for uri in ["/api/tests/nope", "/api/tests/..%2Fetc"] {
        let (status, body) = get(&app, uri).await?;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri}");
        assert_eq!(
            serde_json::from_slice::<ApiError>(&body)?.code,
            ErrorCode::NotFound
        );
    }
    Ok(())
}

#[tokio::test]
async fn a_broken_plugin_is_listed_with_its_error_and_the_server_keeps_working() -> TestResult {
    let dir = tempfile::tempdir()?;
    std::fs::create_dir_all(dir.path().join("bad"))?;
    std::fs::write(dir.path().join("bad/plugin.toml"), "id = 5")?;
    let (app, _site) = app_with(Registry::scan(dir.path()))?;
    let (_, body) = get(&app, "/api/tests").await?;
    let list: TestList = serde_json::from_slice(&body)?;
    assert_eq!(list.tests.len(), 1);
    assert_eq!(list.tests[0].status, TestStatus::Broken);
    assert!(list.tests[0].error.is_some());
    let (status, _) = get(&app, "/api/health").await?;
    assert_eq!(status, StatusCode::OK);
    Ok(())
}

/// An interpreter that can import the `axonhand` package, or `None` (the test is then skipped
/// and says so). Set `AXONHAND_TEST_PYTHON` to choose one.
fn python_with_axonhand() -> Option<PathBuf> {
    let candidates = std::env::var("AXONHAND_TEST_PYTHON")
        .map(|p| vec![p])
        .unwrap_or_else(|_| vec!["python3".into(), "python".into()]);
    candidates.into_iter().map(PathBuf::from).find(|python| {
        Command::new(python)
            .args(["-c", "import axonhand.plugin"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    })
}

#[test]
fn the_example_plugin_runs_end_to_end_with_the_real_python_helpers() -> TestResult {
    let Some(python) = python_with_axonhand() else {
        eprintln!("skipped: no Python with the axonhand package installed (pip install -e python)");
        return Ok(());
    };
    let registry = Registry::scan(&plugins_dir());
    let plugin = registry
        .runnable("example_fixed_values")
        .ok_or("example plug-in not runnable")?;
    let parameters = complete_parameters(
        &plugin.detail,
        &BTreeMap::from([("multiplier".to_owned(), 2.0)]),
    )
    .map_err(|r| r.0)?;
    let job = JobFile {
        schema_version: 1,
        job_id: "e2e".into(),
        test_id: "example_fixed_values".into(),
        video_path: "unused.mp4".into(),
        landmarks_path: None,
        metrics: vec!["example_rate".into(), "example_count".into()],
        parameters,
    };
    let work = tempfile::tempdir()?;
    let result = run(
        &python,
        plugin,
        &job,
        &work.path().join("run"),
        Limits::default(),
    )?
    .result;
    let value = |id: &str| {
        result
            .measures
            .iter()
            .find(|m| m.metric_id == id)
            .and_then(|m| m.value)
    };
    assert_eq!(value("example_rate"), Some(5.0));
    assert_eq!(value("example_count"), Some(12.0));
    assert!(!result.notes.is_empty());
    Ok(())
}

#[test]
fn the_template_becomes_a_valid_plugin_when_copied_under_its_id() -> TestResult {
    let root = tempfile::tempdir()?;
    let target = root.path().join("my_test");
    std::fs::create_dir_all(&target)?;
    for file in ["plugin.toml", "plugin.py"] {
        std::fs::copy(
            plugins_dir().join("_template").join(file),
            target.join(file),
        )?;
    }
    let registry = Registry::scan(root.path());
    assert!(
        registry.runnable("my_test").is_some(),
        "{:?}",
        registry.list()
    );
    // In place the template folder itself is skipped, so it never shows up as a test.
    assert!(Registry::scan(&plugins_dir()).detail("_template").is_none());
    Ok(())
}
