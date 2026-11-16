//! End-to-end test of running a test on a video: a synthetic tapping video is rendered, uploaded,
//! analysed through the job endpoints, and the measures are compared with the generator's truth.
//!
//! It needs a Python with `axonhand` and `av` installed. Set `AXONHAND_PYTHON` to its path; the
//! test is skipped (with a note) when that is not set, so a plain `cargo test` still passes.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axonhand_api::{
    AnalysisResult, ClinicianScore, ErrorCode, Job, JobState, LandmarkOverlay, ResultList,
    ScoreList, Signals, UploadResponse,
};
use axonhand_server::app::{build_jobs_with, router, Services};
use axonhand_server::jobs::{
    choose_extractor, mediapipe_extractor, stand_in_extractor, ExtractFailure, Extractor,
    HandModelChoice,
};
use axonhand_server::probe::python_prober;
use axonhand_server::registry::Registry;
use axonhand_server::rust_hand::{rust_extractor, track_binary_name};
use axonhand_server::store::Store;
use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use tower::ServiceExt;

type TestResult = Result<(), Box<dyn std::error::Error>>;

const BOUNDARY: &str = "XBOUNDARYX";

fn python() -> Option<PathBuf> {
    std::env::var_os("AXONHAND_PYTHON").map(PathBuf::from)
}

fn plugins_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins")
}

struct App {
    router: Router,
    dir: tempfile::TempDir,
    python: PathBuf,
}

fn app(python: PathBuf) -> Result<App, Box<dyn std::error::Error>> {
    let extractor = stand_in_extractor(python.clone());
    app_with(python, extractor)
}

fn app_with(python: PathBuf, extractor: Extractor) -> Result<App, Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    std::fs::create_dir_all(dir.path().join("videos"))?;
    std::fs::create_dir_all(dir.path().join("tmp"))?;
    std::fs::write(dir.path().join("index.html"), "<html></html>")?;
    let store = Arc::new(Store::open_in_memory()?);
    let registry = Arc::new(Registry::scan(&plugins_dir()));
    let jobs = build_jobs_with(
        &store,
        &registry,
        &python,
        &dir.path().join("videos"),
        dir.path(),
        extractor,
    )?;
    jobs.start()?;
    let services = Services {
        store,
        prober: python_prober(python.clone()),
        video_dir: dir.path().join("videos"),
        temp_dir: dir.path().join("tmp"),
        max_upload_bytes: 50 * 1024 * 1024,
        registry,
        jobs,
    };
    Ok(App {
        router: router(dir.path(), services),
        dir,
        python,
    })
}

async fn send(
    app: &Router,
    request: Request<Body>,
) -> Result<(StatusCode, Vec<u8>), Box<dyn std::error::Error>> {
    let response = app.clone().oneshot(request).await?;
    let status = response.status();
    Ok((
        status,
        response.into_body().collect().await?.to_bytes().to_vec(),
    ))
}

async fn get(app: &Router, uri: &str) -> Result<(StatusCode, Vec<u8>), Box<dyn std::error::Error>> {
    send(app, Request::builder().uri(uri).body(Body::empty())?).await
}

async fn post_job(
    app: &Router,
    body: &serde_json::Value,
) -> Result<(StatusCode, Vec<u8>), Box<dyn std::error::Error>> {
    send(
        app,
        Request::builder()
            .method("POST")
            .uri("/api/jobs")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))?,
    )
    .await
}

async fn put_score(
    app: &Router,
    key: &str,
    body: &serde_json::Value,
) -> Result<(StatusCode, Vec<u8>), Box<dyn std::error::Error>> {
    send(
        app,
        Request::builder()
            .method("PUT")
            .uri(format!("/api/results/{key}/scores"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))?,
    )
    .await
}

async fn upload(app: &App, file: &[u8]) -> Result<String, Box<dyn std::error::Error>> {
    let mut body = Vec::new();
    for (name, value) in [
        ("participant_code", "P-001"),
        ("visit_date", "2026-03-01"),
        ("hand", "left"),
    ] {
        body.extend(format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n").bytes());
    }
    body.extend(format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"clip.mp4\"\r\nContent-Type: video/mp4\r\n\r\n").bytes());
    body.extend(file);
    body.extend(format!("\r\n--{BOUNDARY}--\r\n").bytes());
    let (status, bytes) = send(
        &app.router,
        Request::builder()
            .method("POST")
            .uri("/api/videos")
            .header(
                header::CONTENT_TYPE,
                format!("multipart/form-data; boundary={BOUNDARY}"),
            )
            .body(Body::from(body))?,
    )
    .await?;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "{}",
        String::from_utf8_lossy(&bytes)
    );
    Ok(serde_json::from_slice::<UploadResponse>(&bytes)?.video.id)
}

fn render(
    app: &App,
    taps: &str,
    rate: &str,
) -> Result<(Vec<u8>, serde_json::Value), Box<dyn std::error::Error>> {
    let path = app.dir.path().join(format!("synthetic-{taps}-{rate}.mp4"));
    let out = std::process::Command::new(&app.python)
        .args(["-m", "axonhand.tools.make_test_video"])
        .arg(&path)
        .args(["--taps", taps, "--rate", rate])
        .output()?;
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    Ok((std::fs::read(&path)?, serde_json::from_slice(&out.stdout)?))
}

async fn wait_for(app: &Router, id: &str) -> Result<Job, Box<dyn std::error::Error>> {
    wait_up_to(app, id, 30).await
}

async fn wait_up_to(
    app: &Router,
    id: &str,
    seconds: u64,
) -> Result<Job, Box<dyn std::error::Error>> {
    for _ in 0..seconds * 20 {
        let (status, body) = get(app, &format!("/api/jobs/{id}")).await?;
        assert_eq!(status, StatusCode::OK);
        let job: Job = serde_json::from_slice(&body)?;
        if matches!(job.state, JobState::Succeeded | JobState::Failed) {
            return Ok(job);
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    Err(format!("job did not finish in {seconds} seconds").into())
}

fn request(video_id: &str) -> serde_json::Value {
    serde_json::json!({
        "video_id": video_id,
        "test_id": "finger_tapping",
        "metrics": ["tap_count", "tap_rate", "mean_amplitude"],
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn a_video_is_analysed_and_the_answer_is_repeatable() -> TestResult {
    let Some(python) = python() else {
        eprintln!("skipped: set AXONHAND_PYTHON to run this test");
        return Ok(());
    };
    let app = app(python)?;
    let (file, truth) = render(&app, "12", "2.5")?;
    let video_id = upload(&app, &file).await?;

    let (status, body) = post_job(&app.router, &request(&video_id)).await?;
    assert_eq!(
        status,
        StatusCode::ACCEPTED,
        "{}",
        String::from_utf8_lossy(&body)
    );
    let job: Job = serde_json::from_slice(&body)?;
    let done = wait_for(&app.router, &job.id).await?;
    assert_eq!(done.state, JobState::Succeeded, "{:?}", done.error);
    assert_eq!(done.result_key, job.result_key);

    let (status, body) = get(&app.router, &format!("/api/results/{}", done.result_key)).await?;
    assert_eq!(status, StatusCode::OK);
    let result: AnalysisResult = serde_json::from_slice(&body)?;
    assert!(result.illustrative);
    assert!(result.model.stand_in);
    let value = |id: &str| {
        result
            .measures
            .iter()
            .find(|m| m.metric_id == id)
            .and_then(|m| m.value)
    };
    assert_eq!(value("tap_count"), truth["tap_count"].as_f64());
    let rate = value("tap_rate").ok_or("no tap rate")?;
    let expected = truth["tap_rate"].as_f64().ok_or("no truth rate")?;
    assert!(
        (rate - expected).abs() < 0.15,
        "rate {rate}, truth {expected}"
    );

    let (status, body) = get(
        &app.router,
        &format!("/api/results/{}/signals", done.result_key),
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    let signals: Signals = serde_json::from_slice(&body)?;
    assert!(!signals.series.is_empty());

    let (status, body) = get(
        &app.router,
        &format!("/api/results/{}/export.csv", done.result_key),
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    let csv = String::from_utf8(body)?;
    assert!(csv.contains("ILLUSTRATIVE") && csv.contains("tap_rate,"));

    // Asking again returns the same answer without new work.
    let (status, body) = post_job(&app.router, &request(&video_id)).await?;
    assert_eq!(status, StatusCode::OK);
    let again: Job = serde_json::from_slice(&body)?;
    assert_eq!(again.result_key, done.result_key);
    assert_eq!(again.state, JobState::Succeeded);

    // The result is listed, with the video's details, and the filters narrow the list.
    let (status, body) = get(&app.router, "/api/results").await?;
    assert_eq!(status, StatusCode::OK);
    let list: ResultList = serde_json::from_slice(&body)?;
    assert_eq!(list.results.len(), 1);
    assert_eq!(list.results[0].key, done.result_key);
    assert_eq!(list.results[0].video_id, video_id);
    // Clinicians' scores are kept per rater; a second score from the same rater replaces the first.
    let key = &done.result_key;
    let (status, body) = put_score(
        &app.router,
        key,
        &serde_json::json!({"rater": " AB ", "score": 3, "note": "slowing"}),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let saved: ClinicianScore = serde_json::from_slice(&body)?;
    assert_eq!((saved.rater.as_str(), saved.score), ("AB", 3));
    put_score(
        &app.router,
        key,
        &serde_json::json!({"rater": "AB", "score": 2}),
    )
    .await?;
    put_score(
        &app.router,
        key,
        &serde_json::json!({"rater": "CD", "score": 1}),
    )
    .await?;
    let (status, body) = get(&app.router, &format!("/api/results/{key}/scores")).await?;
    assert_eq!(status, StatusCode::OK);
    let scores: ScoreList = serde_json::from_slice(&body)?;
    let shown: Vec<(&str, u8)> = scores
        .scores
        .iter()
        .map(|s| (s.rater.as_str(), s.score))
        .collect();
    assert_eq!(shown, [("AB", 2), ("CD", 1)]);
    for bad in [
        serde_json::json!({"rater": "AB", "score": 5}),
        serde_json::json!({"rater": "", "score": 1}),
        serde_json::json!({"score": 1}),
    ] {
        let (status, _) = put_score(&app.router, key, &bad).await?;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}");
    }
    let unknown = "0".repeat(64);
    let (status, _) = put_score(
        &app.router,
        &unknown,
        &serde_json::json!({"rater": "AB", "score": 1}),
    )
    .await?;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = get(&app.router, &format!("/api/results/{unknown}/scores")).await?;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (_, body) = get(&app.router, "/api/results?test_id=no_such_test").await?;
    let none: ResultList = serde_json::from_slice(&body)?;
    assert!(none.results.is_empty());
    let (_, body) = get(&app.router, "/api/results?test_id=&participant_code=").await?;
    let all: ResultList = serde_json::from_slice(&body)?;
    assert_eq!(all.results.len(), 1);
    Ok(())
}

/// The stand-in tracker, wrapped so that it also writes the file the real model writes for
/// drawing landmarks over the video.
fn extractor_with_overlay(python: PathBuf) -> Extractor {
    let inner = stand_in_extractor(python);
    let run = Arc::clone(&inner.run);
    Extractor {
        run: Arc::new(move |video, output, device| {
            run(video, output, device)?;
            std::fs::write(
                output.with_extension("json"),
                br#"{"fps":30.0,"points":[null,[[1.5,2.5],null,[3.0,4.0]]]}"#,
            )
            .map_err(|e| ExtractFailure::Broken(e.to_string()))
        }),
        ..inner
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn landmarks_for_drawing_are_saved_with_the_result_when_the_tool_writes_them() -> TestResult {
    let Some(python) = python() else {
        eprintln!("skipped: set AXONHAND_PYTHON to run this test");
        return Ok(());
    };
    let (file, _) = render(&app(python.clone())?, "8", "2.0")?;

    let with = app_with(python.clone(), extractor_with_overlay(python.clone()))?;
    let video_id = upload(&with, &file).await?;
    let (_, body) = post_job(&with.router, &request(&video_id)).await?;
    let job: Job = serde_json::from_slice(&body)?;
    let done = wait_for(&with.router, &job.id).await?;
    assert_eq!(done.state, JobState::Succeeded, "{:?}", done.error);
    let (status, body) = get(
        &with.router,
        &format!("/api/results/{}/landmarks", done.result_key),
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    let overlay: LandmarkOverlay = serde_json::from_slice(&body)?;
    assert_eq!(overlay.fps, 30.0);
    assert_eq!(overlay.points.len(), 2);
    assert!(overlay.points[0].is_none());
    assert_eq!(
        overlay.points[1].as_ref().map(|f| f[0]),
        Some(Some([1.5, 2.5]))
    );

    // A tool that writes no overlay still gives a result; only the overlay is missing.
    let inner = stand_in_extractor(python.clone());
    let run = Arc::clone(&inner.run);
    let silent = Extractor {
        run: Arc::new(move |video, output, device| {
            run(video, output, device)?;
            let _ = std::fs::remove_file(output.with_extension("json"));
            Ok(())
        }),
        ..inner
    };
    let without = app_with(python, silent)?;
    let video_id = upload(&without, &file).await?;
    let (_, body) = post_job(&without.router, &request(&video_id)).await?;
    let job: Job = serde_json::from_slice(&body)?;
    let done = wait_for(&without.router, &job.id).await?;
    assert_eq!(done.state, JobState::Succeeded);
    let (status, _) = get(
        &without.router,
        &format!("/api/results/{}/landmarks", done.result_key),
    )
    .await?;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = get(&without.router, "/api/results/not-a-key/landmarks").await?;
    assert_eq!(status, StatusCode::NOT_FOUND);
    Ok(())
}

/// The stand-in tracker presented as a real model, that records where each run was asked to go.
fn extractor_that_records_the_device(
    python: PathBuf,
    asked: Arc<std::sync::Mutex<Vec<axonhand_api::ComputeDevice>>>,
) -> Extractor {
    let inner = stand_in_extractor(python);
    let run = Arc::clone(&inner.run);
    let mut model = inner.model.clone();
    model.stand_in = false;
    model.name = "Recording model".to_owned();
    Extractor {
        model,
        run: Arc::new(move |video, output, device| {
            if let Ok(mut list) = asked.lock() {
                list.push(device);
            }
            run(video, output, device)
        }),
        ..inner
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_cpu_and_the_npu_give_separate_results_that_say_where_they_ran() -> TestResult {
    let Some(python) = python() else {
        eprintln!("skipped: set AXONHAND_PYTHON to run this test");
        return Ok(());
    };
    let asked = Arc::new(std::sync::Mutex::new(Vec::new()));
    let app = app_with(
        python.clone(),
        extractor_that_records_the_device(python.clone(), Arc::clone(&asked)),
    )?;
    let (file, _) = render(&app, "8", "2.0")?;
    let video_id = upload(&app, &file).await?;

    // Nothing is said about the device: that is the CPU.
    let (_, body) = post_job(&app.router, &request(&video_id)).await?;
    let cpu_job = wait_for(&app.router, &serde_json::from_slice::<Job>(&body)?.id).await?;
    let mut npu_request = request(&video_id);
    npu_request["device"] = "npu".into();
    let (_, body) = post_job(&app.router, &npu_request).await?;
    let npu_job = wait_for(&app.router, &serde_json::from_slice::<Job>(&body)?.id).await?;
    assert_eq!(cpu_job.state, JobState::Succeeded, "{:?}", cpu_job.error);
    assert_eq!(npu_job.state, JobState::Succeeded, "{:?}", npu_job.error);
    assert_ne!(cpu_job.result_key, npu_job.result_key);

    let result = |key: String| {
        let router = app.router.clone();
        async move {
            let (_, body) = get(&router, &format!("/api/results/{key}")).await?;
            Ok::<AnalysisResult, Box<dyn std::error::Error>>(serde_json::from_slice(&body)?)
        }
    };
    let (cpu, npu) = (
        result(cpu_job.result_key).await?,
        result(npu_job.result_key).await?,
    );
    assert_eq!(
        (cpu.model.device.as_str(), npu.model.device.as_str()),
        ("CPU", "NPU")
    );
    assert_ne!(cpu.model.hash, npu.model.hash);
    assert_eq!(
        *asked.lock().map_err(|e| e.to_string())?,
        [
            axonhand_api::ComputeDevice::Cpu,
            axonhand_api::ComputeDevice::Npu
        ]
    );

    // Asking again for the CPU repeats nothing.
    let (_, body) = post_job(&app.router, &request(&video_id)).await?;
    assert_eq!(
        serde_json::from_slice::<Job>(&body)?.state,
        JobState::Succeeded
    );
    assert_eq!(asked.lock().map_err(|e| e.to_string())?.len(), 2);
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn bad_requests_get_clear_errors() -> TestResult {
    let Some(python) = python() else {
        eprintln!("skipped: set AXONHAND_PYTHON to run this test");
        return Ok(());
    };
    let app = app(python)?;
    let (file, _) = render(&app, "8", "2.0")?;
    let video_id = upload(&app, &file).await?;

    let (status, body) = post_job(&app.router, &request(&"0".repeat(64))).await?;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "{}",
        String::from_utf8_lossy(&body)
    );
    let error: axonhand_api::ApiError = serde_json::from_slice(&body)?;
    assert_eq!(error.code, ErrorCode::NotFound);

    let mut unknown_test = request(&video_id);
    unknown_test["test_id"] = "nonsense".into();
    assert_eq!(
        post_job(&app.router, &unknown_test).await?.0,
        StatusCode::BAD_REQUEST
    );

    let mut no_metrics = request(&video_id);
    no_metrics["metrics"] = serde_json::json!([]);
    assert_eq!(
        post_job(&app.router, &no_metrics).await?.0,
        StatusCode::BAD_REQUEST
    );

    let mut bad_setting = request(&video_id);
    bad_setting["parameters"] = serde_json::json!({"min_amplitude": 99.0});
    assert_eq!(
        post_job(&app.router, &bad_setting).await?.0,
        StatusCode::BAD_REQUEST
    );

    let (status, _) = send(
        &app.router,
        Request::builder()
            .method("POST")
            .uri("/api/jobs")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from("not json"))?,
    )
    .await?;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    assert_eq!(
        get(&app.router, "/api/jobs/nope").await?.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        get(&app.router, &format!("/api/results/{}", "a".repeat(64)))
            .await?
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        get(&app.router, "/api/results/..%2Fsecret").await?.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        get(&app.router, "/api/results/short/export.csv").await?.0,
        StatusCode::NOT_FOUND
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_file_that_is_not_a_hand_video_fails_with_a_reason() -> TestResult {
    let Some(python) = python() else {
        eprintln!("skipped: set AXONHAND_PYTHON to run this test");
        return Ok(());
    };
    let app = app(python)?;
    // A real video with no coloured dots: the stand-in model finds no hand.
    let blank = app.dir.path().join("blank.mp4");
    let script = "import av,numpy as np,sys\nc=av.open(sys.argv[1],'w')\ns=c.add_stream('libx264',rate=30)\ns.width,s.height,s.pix_fmt=320,240,'yuv420p'\nfor _ in range(60):\n    for p in s.encode(av.VideoFrame.from_ndarray(np.full((240,320,3),200,np.uint8),format='rgb24')): c.mux(p)\nfor p in s.encode(): c.mux(p)\nc.close()";
    let out = std::process::Command::new(&app.python)
        .args(["-c", script])
        .arg(&blank)
        .output()?;
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let video_id = upload(&app, &std::fs::read(&blank)?).await?;
    let (status, body) = post_job(&app.router, &request(&video_id)).await?;
    assert_eq!(
        status,
        StatusCode::ACCEPTED,
        "{}",
        String::from_utf8_lossy(&body)
    );
    let job: Job = serde_json::from_slice(&body)?;
    let done = wait_for(&app.router, &job.id).await?;
    assert_eq!(done.state, JobState::Failed);
    assert!(done.error.is_some_and(|e| !e.is_empty()));
    assert_eq!(
        get(&app.router, &format!("/api/results/{}", done.result_key))
            .await?
            .0,
        StatusCode::NOT_FOUND
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn the_real_hand_model_says_plainly_when_there_is_no_hand() -> TestResult {
    let Some(python) = python() else {
        eprintln!("skipped: set AXONHAND_PYTHON to run this test");
        return Ok(());
    };
    let models = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../models");
    let Ok(extractor) = mediapipe_extractor(python.clone(), &models) else {
        eprintln!("skipped: the hand model files are not in the models folder");
        return Ok(());
    };
    let app = app_with(python, extractor)?;
    // Coloured dots on grey are not a hand, however a stand-in might read them.
    let (file, _) = render(&app, "8", "2.0")?;
    let video_id = upload(&app, &file).await?;
    let (status, body) = post_job(&app.router, &request(&video_id)).await?;
    assert_eq!(
        status,
        StatusCode::ACCEPTED,
        "{}",
        String::from_utf8_lossy(&body)
    );
    let job: Job = serde_json::from_slice(&body)?;
    let done = wait_for(&app.router, &job.id).await?;
    assert_eq!(done.state, JobState::Failed);
    let reason = done.error.unwrap_or_default();
    assert!(reason.contains("No hand was found"), "{reason}");
    Ok(())
}

#[test]
fn auto_prefers_the_rust_tracker_when_it_can_run() -> TestResult {
    let Some(python) = python() else {
        eprintln!("skipped: set AXONHAND_PYTHON to run this test");
        return Ok(());
    };
    let models = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../models");
    let tracker = std::env::var_os("AXONHAND_TRACK").map_or_else(
        || {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/debug")
                .join(track_binary_name())
        },
        PathBuf::from,
    );
    if !tracker.is_file()
        || !models
            .join("palm_detection_mediapipe_2023feb.onnx")
            .is_file()
    {
        eprintln!("skipped: needs the tracker program and the model files");
        return Ok(());
    }
    let chosen = choose_extractor(HandModelChoice::Auto, &python, &models, &tracker)?;
    assert!(chosen.model.name.contains("Rust"), "{}", chosen.model.name);
    // The same files without the tracker program fall back to the Python pipeline.
    let without = choose_extractor(
        HandModelChoice::Auto,
        &python,
        &models,
        Path::new("no-such-tracker-program"),
    )?;
    assert!(!without.model.name.contains("Rust") && !without.model.stand_in);
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn the_rust_tracker_says_plainly_when_there_is_no_hand_and_the_decoder_when_it_is_no_video(
) -> TestResult {
    let Some(python) = python() else {
        eprintln!("skipped: set AXONHAND_PYTHON to run this test");
        return Ok(());
    };
    let models = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../models");
    let tracker = std::env::var_os("AXONHAND_TRACK").map_or_else(
        || {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/debug")
                .join(track_binary_name())
        },
        PathBuf::from,
    );
    if !tracker.is_file() {
        eprintln!("skipped: build the tracker first (cargo build -p axonhand-track)");
        return Ok(());
    }
    let Ok(extractor) = rust_extractor(python.clone(), &models, &tracker) else {
        eprintln!("skipped: the hand model files are not in the models folder");
        return Ok(());
    };
    assert!(!extractor.model.stand_in);
    assert!(extractor.model.name.contains("Rust"));
    let python_model = mediapipe_extractor(python.clone(), &models)?;
    assert_ne!(
        extractor.model.hash, python_model.model.hash,
        "results of the two trackers must never share a key"
    );
    let app = app_with(python, extractor)?;
    // A short video: the tracker here is an unoptimised build on the CPU, and with no hand in
    // view it looks for one in every frame.
    let (file, _) = render(&app, "2", "2.0")?;
    let video_id = upload(&app, &file).await?;
    let (_, body) = post_job(&app.router, &request(&video_id)).await?;
    let job: Job = serde_json::from_slice(&body)?;
    let done = wait_up_to(&app.router, &job.id, 180).await?;
    assert_eq!(done.state, JobState::Failed);
    let reason = done.error.unwrap_or_default();
    assert!(reason.contains("No hand was found"), "{reason}");
    Ok(())
}
