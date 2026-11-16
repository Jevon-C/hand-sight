//! The HTTP application: routes, layers and static files.

use std::path::Path;
use std::sync::Arc;

use axonhand_api::{Health, HealthStatus};
use axum::extract::DefaultBodyLimit;
use axum::http::HeaderName;
use axum::routing::{get, post};
use axum::{Json, Router};
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;

use crate::analysis_api;
use crate::config::Config;
use crate::error::ServerError;
use crate::jobs::{choose_extractor, stand_in_extractor, Extractor, JobContext, JobManager};
use crate::probe::Prober;
use crate::registry::Registry;
use crate::results::ResultFiles;
use crate::runner::Limits;
use crate::store::Store;
use crate::tests_api;
use crate::videos::{self, VideoState};

const REQUEST_ID: &str = "x-request-id";

/// What the application needs besides the built web app.
pub struct Services {
    /// The video index.
    pub store: Arc<Store>,
    /// Reads video facts.
    pub prober: Prober,
    /// Folder for stored videos.
    pub video_dir: std::path::PathBuf,
    /// Folder for uploads in progress.
    pub temp_dir: std::path::PathBuf,
    /// Largest accepted upload in bytes.
    pub max_upload_bytes: u64,
    /// The test plug-ins found at start-up.
    pub registry: Arc<Registry>,
    /// Runs tests on videos and holds the saved results.
    pub jobs: Arc<JobManager>,
}

impl Services {
    /// Prepare the data folder described by `config`: create its sub-folders, open the index,
    /// and clear uploads left unfinished by an earlier run.
    ///
    /// # Errors
    ///
    /// A sentence describing what could not be set up.
    pub fn from_config(config: &Config) -> Result<Self, String> {
        let video_dir = config.data_dir.join("videos");
        let temp_dir = config.data_dir.join("tmp");
        for dir in [&video_dir, &temp_dir] {
            std::fs::create_dir_all(dir)
                .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
        }
        if let Ok(entries) = std::fs::read_dir(&temp_dir) {
            for entry in entries.flatten() {
                // Best effort: these are partial uploads from a run that was interrupted.
                let _ = std::fs::remove_file(entry.path());
            }
        }
        let index = config.data_dir.join("index.sqlite");
        let store = Arc::new(
            Store::open(&index).map_err(|e| format!("cannot open {}: {e}", index.display()))?,
        );
        let registry = Arc::new(Registry::scan(&config.plugins_dir));
        let extractor = choose_extractor(
            config.hand_model,
            &config.python,
            &config.models_dir,
            &config.track_binary,
        )?;
        tracing::info!(
            model = %extractor.model.name,
            stand_in = extractor.model.stand_in,
            "hand model chosen"
        );
        let jobs = build_jobs_with(
            &store,
            &registry,
            &config.python,
            &video_dir,
            &config.data_dir,
            extractor,
        )?;
        Ok(Self {
            store,
            prober: crate::probe::python_prober(config.python.clone()),
            video_dir,
            temp_dir,
            max_upload_bytes: config.max_upload_bytes,
            registry,
            jobs,
        })
    }
}

/// Build the job manager with the development stand-in hand model. Its worker is started
/// separately with [`JobManager::start`].
///
/// # Errors
///
/// A sentence describing what could not be set up.
pub fn build_jobs(
    store: &Arc<Store>,
    registry: &Arc<Registry>,
    python: &Path,
    video_dir: &Path,
    data_dir: &Path,
) -> Result<Arc<JobManager>, String> {
    let extractor = stand_in_extractor(python.to_path_buf());
    build_jobs_with(store, registry, python, video_dir, data_dir, extractor)
}

/// Like [`build_jobs`], with the hand model given.
///
/// # Errors
///
/// A sentence describing what could not be set up.
pub fn build_jobs_with(
    store: &Arc<Store>,
    registry: &Arc<Registry>,
    python: &Path,
    video_dir: &Path,
    data_dir: &Path,
    extractor: Extractor,
) -> Result<Arc<JobManager>, String> {
    let work_dir = data_dir.join("work");
    std::fs::create_dir_all(&work_dir)
        .map_err(|e| format!("cannot create {}: {e}", work_dir.display()))?;
    let results = ResultFiles::open(&data_dir.join("results")).map_err(|e| e.to_string())?;
    Ok(JobManager::new(JobContext {
        store: Arc::clone(store),
        results,
        registry: Arc::clone(registry),
        extractor,
        python: python.to_path_buf(),
        video_dir: video_dir.to_path_buf(),
        work_dir,
        limits: Limits::default(),
    }))
}

/// Build the router. API routes live under `/api`; everything else is served from
/// `static_dir`, falling back to its `index.html` so the browser app can handle its own routes.
pub fn router(static_dir: &Path, services: Services) -> Router {
    let request_id = HeaderName::from_static(REQUEST_ID);
    let files = ServeDir::new(static_dir).fallback(ServeFile::new(static_dir.join("index.html")));
    let body_limit =
        usize::try_from(services.max_upload_bytes.saturating_add(64 * 1024)).unwrap_or(usize::MAX);
    let registry = services.registry;
    let jobs = services.jobs;
    let state = VideoState {
        store: services.store,
        prober: services.prober,
        video_dir: services.video_dir,
        temp_dir: services.temp_dir,
        max_upload_bytes: services.max_upload_bytes,
    };
    let video_routes = Router::new()
        .route("/api/videos", post(videos::upload).get(videos::list))
        .route("/api/videos/{id}", get(videos::get_one))
        .route("/api/videos/{id}/stream", get(videos::stream))
        .layer(DefaultBodyLimit::max(body_limit))
        .with_state(state);
    Router::new()
        .route("/api/health", get(health))
        .merge(video_routes)
        .route("/api/tests", get(tests_api::list))
        .route("/api/tests/{id}", get(tests_api::get_one))
        .with_state(registry)
        .merge(analysis_routes(jobs))
        .route(
            "/api/{*rest}",
            get(unknown_api).post(unknown_api).put(unknown_api),
        )
        .fallback_service(files)
        .layer(TraceLayer::new_for_http())
        .layer(PropagateRequestIdLayer::new(request_id.clone()))
        .layer(SetRequestIdLayer::new(request_id, MakeRequestUuid))
}

fn analysis_routes(jobs: Arc<JobManager>) -> Router {
    Router::new()
        .route("/api/jobs", post(analysis_api::create_job))
        .route("/api/jobs/{id}", get(analysis_api::get_job))
        .route("/api/jobs/{id}/events", get(analysis_api::job_events))
        .route("/api/results", get(analysis_api::list_results))
        .route("/api/results/{key}", get(analysis_api::get_result))
        .route("/api/results/{key}/signals", get(analysis_api::get_signals))
        .route(
            "/api/results/{key}/scores",
            get(analysis_api::list_scores).put(analysis_api::put_score),
        )
        .route(
            "/api/results/{key}/landmarks",
            get(analysis_api::get_landmarks),
        )
        .route(
            "/api/results/{key}/export.csv",
            get(analysis_api::export_csv),
        )
        .with_state(jobs)
}

async fn health() -> Json<Health> {
    Json(Health {
        status: HealthStatus::Ok,
        version: env!("CARGO_PKG_VERSION").to_owned(),
    })
}

/// Unknown `/api` paths get the shared error shape, not the app's HTML page.
async fn unknown_api() -> ServerError {
    ServerError::NotFound("No such API endpoint.".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    async fn get_path(
        app: Router,
        path: &str,
    ) -> Result<(StatusCode, String, Option<String>), Box<dyn std::error::Error>> {
        let response = app
            .oneshot(Request::builder().uri(path).body(Body::empty())?)
            .await?;
        let status = response.status();
        let id = response
            .headers()
            .get(REQUEST_ID)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let bytes = response.into_body().collect().await?.to_bytes();
        Ok((status, String::from_utf8(bytes.to_vec())?, id))
    }

    fn test_services(root: &Path) -> Result<Services, Box<dyn std::error::Error>> {
        let store = Arc::new(Store::open_in_memory()?);
        let registry = Arc::new(Registry::default());
        let jobs = build_jobs(
            &store,
            &registry,
            Path::new("python"),
            &root.join("videos"),
            root,
        )
        .map_err(std::io::Error::other)?;
        Ok(Services {
            store,
            prober: Arc::new(|_| Err(crate::probe::ProbeFailure::Broken("unused".into()))),
            video_dir: root.join("videos"),
            temp_dir: root.join("tmp"),
            max_upload_bytes: 1000,
            registry,
            jobs,
        })
    }

    fn site() -> Result<tempfile::TempDir, std::io::Error> {
        let dir = tempfile::tempdir()?;
        std::fs::write(dir.path().join("index.html"), "<html>app</html>")?;
        std::fs::write(dir.path().join("app.js"), "console.log(1)")?;
        Ok(dir)
    }

    #[tokio::test]
    async fn health_reports_ok_and_the_version() -> Result<(), Box<dyn std::error::Error>> {
        let dir = site()?;
        let (status, body, _) = get_path(
            router(dir.path(), test_services(dir.path())?),
            "/api/health",
        )
        .await?;
        assert_eq!(status, StatusCode::OK);
        let health: Health = serde_json::from_str(&body)?;
        assert_eq!(health.status, HealthStatus::Ok);
        assert_eq!(health.version, env!("CARGO_PKG_VERSION"));
        Ok(())
    }

    #[tokio::test]
    async fn unknown_api_paths_return_the_shared_error() -> Result<(), Box<dyn std::error::Error>> {
        let dir = site()?;
        let (status, body, _) = get_path(
            router(dir.path(), test_services(dir.path())?),
            "/api/nothing/here",
        )
        .await?;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let error: axonhand_api::ApiError = serde_json::from_str(&body)?;
        assert_eq!(error.code, axonhand_api::ErrorCode::NotFound);
        Ok(())
    }

    #[tokio::test]
    async fn static_files_are_served_and_other_paths_fall_back_to_the_app(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let dir = site()?;
        let (status, body, _) =
            get_path(router(dir.path(), test_services(dir.path())?), "/app.js").await?;
        assert_eq!((status, body.as_str()), (StatusCode::OK, "console.log(1)"));
        let (status, body, _) = get_path(
            router(dir.path(), test_services(dir.path())?),
            "/results/abc",
        )
        .await?;
        assert_eq!(
            (status, body.as_str()),
            (StatusCode::OK, "<html>app</html>")
        );
        Ok(())
    }

    #[tokio::test]
    async fn every_response_carries_a_request_id() -> Result<(), Box<dyn std::error::Error>> {
        let dir = site()?;
        let (_, _, id) = get_path(
            router(dir.path(), test_services(dir.path())?),
            "/api/health",
        )
        .await?;
        assert!(id.is_some_and(|v| !v.is_empty()));
        Ok(())
    }
}
