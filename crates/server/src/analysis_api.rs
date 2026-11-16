//! Endpoints for running tests and reading results.

use std::convert::Infallible;
use std::sync::Arc;

use axonhand_api::{
    AnalysisResult, ClinicianScore, Job, JobRequest, JobState, ResultList, ScoreList, ScoreRequest,
    Signals,
};
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::Json;
use futures_util::stream::{self, Stream};
use serde::Deserialize;
use tokio::sync::broadcast::error::RecvError;

use crate::csv_export::result_csv;
use crate::error::ServerError;
use crate::jobs::JobManager;
use crate::media::is_video_id;
use crate::results::ResultError;

/// Shared state of the analysis endpoints.
pub type AnalysisState = Arc<JobManager>;

fn not_found(what: &str) -> ServerError {
    ServerError::NotFound(format!("No such {what}."))
}

impl From<ResultError> for ServerError {
    fn from(error: ResultError) -> Self {
        match error {
            ResultError::BadKey => not_found("result"),
            other => Self::Internal(other.to_string()),
        }
    }
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, ServerError> + Send + 'static,
) -> Result<T, ServerError> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| ServerError::Internal(format!("background task failed: {e}")))?
}

/// `POST /api/jobs`: `202` when work was queued, `200` when the result already exists or the
/// same work is already waiting.
pub async fn create_job(
    State(jobs): State<AnalysisState>,
    body: Result<Json<JobRequest>, JsonRejection>,
) -> Result<Response, ServerError> {
    let Json(request) = body
        .map_err(|_| ServerError::InvalidRequest("The request body is not a valid job.".into()))?;
    let (job, queued) = blocking(move || jobs.submit(&request)).await?;
    let status = if queued {
        StatusCode::ACCEPTED
    } else {
        StatusCode::OK
    };
    Ok((status, Json(job)).into_response())
}

/// `GET /api/jobs/{id}`
pub async fn get_job(
    State(jobs): State<AnalysisState>,
    Path(id): Path<String>,
) -> Result<Json<Job>, ServerError> {
    blocking(move || jobs.job(&id))
        .await?
        .map(Json)
        .ok_or_else(|| not_found("job"))
}

fn is_finished(job: &Job) -> bool {
    matches!(job.state, JobState::Succeeded | JobState::Failed)
}

fn event(job: &Job) -> Event {
    Event::default()
        .event("job")
        .json_data(job)
        .unwrap_or_else(|_| Event::default().event("job").data("{}"))
}

/// `GET /api/jobs/{id}/events`: a server-sent event stream. It sends the job's current state at
/// once, then every change, and ends after the job succeeds or fails.
pub async fn job_events(
    State(jobs): State<AnalysisState>,
    Path(id): Path<String>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ServerError> {
    // Subscribe before reading, so a change between the two cannot be missed.
    let receiver = jobs.subscribe();
    let manager = Arc::clone(&jobs);
    let lookup = id.clone();
    let first = blocking(move || manager.job(&lookup))
        .await?
        .ok_or_else(|| not_found("job"))?;
    let done = is_finished(&first);
    let state = (Some(first), receiver, jobs, id, done);
    let stream = stream::unfold(state, |(first, mut receiver, jobs, id, done)| async move {
        if let Some(job) = first {
            return Some((Ok(event(&job)), (None, receiver, jobs, id, done)));
        }
        if done {
            return None;
        }
        loop {
            match receiver.recv().await {
                Ok(job) if job.id == id => {
                    let finished = is_finished(&job);
                    return Some((Ok(event(&job)), (None, receiver, jobs, id, finished)));
                }
                Ok(_) => {}
                Err(RecvError::Lagged(_)) => {
                    // Missed some changes: send the current state instead.
                    let manager = Arc::clone(&jobs);
                    let lookup = id.clone();
                    let current = blocking(move || manager.job(&lookup))
                        .await
                        .ok()
                        .flatten()?;
                    let finished = is_finished(&current);
                    return Some((Ok(event(&current)), (None, receiver, jobs, id, finished)));
                }
                Err(RecvError::Closed) => return None,
            }
        }
    });
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

fn checked_key(key: &str) -> Result<(), ServerError> {
    if is_video_id(key) {
        Ok(())
    } else {
        Err(not_found("result"))
    }
}

/// Query string of `GET /api/results`.
#[derive(Debug, Deserialize)]
pub struct ResultsQuery {
    test_id: Option<String>,
    participant_code: Option<String>,
}

/// `GET /api/results`: saved results, newest first.
pub async fn list_results(
    State(jobs): State<AnalysisState>,
    Query(query): Query<ResultsQuery>,
) -> Result<Json<ResultList>, ServerError> {
    let results = blocking(move || {
        jobs.list_results(
            query.test_id.as_deref().filter(|t| !t.is_empty()),
            query.participant_code.as_deref().filter(|c| !c.is_empty()),
        )
    })
    .await?;
    Ok(Json(ResultList { results }))
}

/// `GET /api/results/{key}`
pub async fn get_result(
    State(jobs): State<AnalysisState>,
    Path(key): Path<String>,
) -> Result<Json<AnalysisResult>, ServerError> {
    checked_key(&key)?;
    let found = blocking(move || Ok(jobs.results().read(&key)?)).await?;
    found.map(Json).ok_or_else(|| not_found("result"))
}

fn known_result(jobs: &Arc<JobManager>, key: &str) -> Result<(), ServerError> {
    checked_key(key)?;
    if jobs.result_exists(key)? {
        Ok(())
    } else {
        Err(not_found("result"))
    }
}

/// `GET /api/results/{key}/scores`
pub async fn list_scores(
    State(jobs): State<AnalysisState>,
    Path(key): Path<String>,
) -> Result<Json<ScoreList>, ServerError> {
    let scores = blocking(move || {
        known_result(&jobs, &key)?;
        jobs.scores(&key)
    })
    .await?;
    Ok(Json(ScoreList { scores }))
}

/// `PUT /api/results/{key}/scores`: save one clinician's score, replacing that rater's earlier one.
pub async fn put_score(
    State(jobs): State<AnalysisState>,
    Path(key): Path<String>,
    body: Result<Json<ScoreRequest>, JsonRejection>,
) -> Result<Json<ClinicianScore>, ServerError> {
    let Json(request) = body.map_err(|_| {
        ServerError::InvalidRequest("The request body is not a valid score.".into())
    })?;
    let request = request.validated().map_err(ServerError::Field)?;
    let saved = blocking(move || {
        known_result(&jobs, &key)?;
        jobs.put_score(&key, &request)
    })
    .await?;
    Ok(Json(saved))
}

/// `GET /api/results/{key}/signals`
pub async fn get_signals(
    State(jobs): State<AnalysisState>,
    Path(key): Path<String>,
) -> Result<Json<Signals>, ServerError> {
    checked_key(&key)?;
    let found = blocking(move || Ok(jobs.results().read_signals(&key)?)).await?;
    found.map(Json).ok_or_else(|| not_found("signals"))
}

/// `GET /api/results/{key}/landmarks`: the hand landmarks of every frame, for drawing over the
/// video (see [`axonhand_api::LandmarkOverlay`]).
pub async fn get_landmarks(
    State(jobs): State<AnalysisState>,
    Path(key): Path<String>,
) -> Result<Response, ServerError> {
    checked_key(&key)?;
    let found = blocking(move || Ok(jobs.results().read_landmarks(&key)?)).await?;
    let bytes = found.ok_or_else(|| not_found("landmarks"))?;
    Ok(([(header::CONTENT_TYPE, "application/json")], bytes).into_response())
}

/// `GET /api/results/{key}/export.csv`
pub async fn export_csv(
    State(jobs): State<AnalysisState>,
    Path(key): Path<String>,
) -> Result<Response, ServerError> {
    checked_key(&key)?;
    let short = key.chars().take(12).collect::<String>();
    let result = blocking(move || Ok(jobs.results().read(&key)?))
        .await?
        .ok_or_else(|| not_found("result"))?;
    Ok((
        [
            (header::CONTENT_TYPE, "text/csv; charset=utf-8".to_owned()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"axonhand-result-{short}.csv\""),
            ),
        ],
        result_csv(&result),
    )
        .into_response())
}
