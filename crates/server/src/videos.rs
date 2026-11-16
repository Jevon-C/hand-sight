//! Video endpoints: upload, list, describe and stream.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axonhand_api::{ApiError, Hand, UploadMeta, UploadResponse, Video, VideoList};
use axum::body::Body;
use axum::extract::multipart::{Field, Multipart};
use axum::extract::{Path as UrlPath, Query, State};
use axum::http::{Request, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tower::ServiceExt;
use tower_http::services::ServeFile;

use crate::clock::now_utc;
use crate::error::ServerError;
use crate::gate;
use crate::media::{display_name, hex, is_video_id, Container};
use crate::probe::{ProbeFailure, Prober};
use crate::store::{Store, StoreError, VideoFilter};

/// Everything the video handlers share.
#[derive(Clone)]
pub struct VideoState {
    /// The index.
    pub store: Arc<Store>,
    /// Reads a video's facts.
    pub prober: Prober,
    /// Folder holding the stored video files.
    pub video_dir: PathBuf,
    /// Folder for uploads still being received. On the same disk as `video_dir`, so the final
    /// move is a rename.
    pub temp_dir: PathBuf,
    /// Largest accepted upload in bytes.
    pub max_upload_bytes: u64,
}

impl From<StoreError> for ServerError {
    fn from(error: StoreError) -> Self {
        Self::Internal(error.to_string())
    }
}

/// Run blocking work off the async threads.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, ServerError> + Send + 'static,
) -> Result<T, ServerError> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| ServerError::Internal(format!("background task failed: {e}")))?
}

/// Text of a small form field, refusing anything unreasonably long.
async fn field_text(field: Field<'_>) -> Result<String, ServerError> {
    let name = field.name().unwrap_or("").to_owned();
    let text = field.text().await.map_err(|_| {
        ServerError::Field(ApiError::invalid(
            name.clone(),
            "Could not read this field.",
        ))
    })?;
    if text.len() > 1024 {
        return Err(ServerError::Field(ApiError::invalid(
            name,
            "This value is too long.",
        )));
    }
    Ok(text)
}

fn parse_hand(text: &str) -> Result<Hand, ServerError> {
    match text.trim() {
        "left" => Ok(Hand::Left),
        "right" => Ok(Hand::Right),
        _ => Err(ServerError::Field(ApiError::invalid(
            "hand",
            "Choose left or right.",
        ))),
    }
}

/// A file received into the temporary folder.
struct Received {
    path: PathBuf,
    id: String,
    size: u64,
    filename: String,
    container: Container,
}

/// Remove a temporary file when dropped, unless it was moved away.
struct TempGuard(Option<PathBuf>);

impl TempGuard {
    fn disarm(&mut self) {
        self.0 = None;
    }
}

impl Drop for TempGuard {
    fn drop(&mut self) {
        if let Some(path) = self.0.take() {
            // Best effort: a leftover temp file is harmless and cleared on the next start.
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Stream the `file` field to disk, hashing as it arrives and enforcing the size limit.
async fn receive_file(
    mut field: Field<'_>,
    temp_dir: &Path,
    limit: u64,
    guard: &mut TempGuard,
) -> Result<Received, ServerError> {
    let filename = display_name(field.file_name().unwrap_or(""));
    let path = temp_dir.join(format!("upload-{}.part", hex(&rand_bytes())));
    guard.0 = Some(path.clone());
    let mut file = tokio::fs::File::create(&path)
        .await
        .map_err(|e| ServerError::Internal(format!("cannot create temp file: {e}")))?;
    let mut hasher = Sha256::new();
    let mut size: u64 = 0;
    let mut head: Vec<u8> = Vec::with_capacity(16);
    while let Some(chunk) = field.chunk().await.map_err(|_| {
        ServerError::InvalidRequest("The upload was interrupted or malformed.".into())
    })? {
        size += chunk.len() as u64;
        if size > limit {
            return Err(ServerError::TooLarge(format!(
                "The video is larger than the {} MB limit.",
                limit / 1_000_000
            )));
        }
        if head.len() < 16 {
            head.extend(chunk.iter().take(16 - head.len()));
        }
        hasher.update(&chunk);
        file.write_all(&chunk)
            .await
            .map_err(|e| ServerError::Internal(format!("cannot write upload: {e}")))?;
    }
    file.flush()
        .await
        .map_err(|e| ServerError::Internal(format!("cannot write upload: {e}")))?;
    drop(file);
    if size == 0 {
        return Err(ServerError::Field(ApiError::invalid(
            "file",
            "The file is empty.",
        )));
    }
    let container = Container::sniff(&head).ok_or_else(|| {
        ServerError::UnsupportedMedia("This is not an MP4, MOV or WebM video.".into())
    })?;
    Ok(Received {
        path,
        id: hex(&hasher.finalize()),
        size,
        filename,
        container,
    })
}

/// Unpredictable bytes for temporary file names, from the standard library's randomised hasher.
fn rand_bytes() -> [u8; 8] {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let mut hasher = RandomState::new().build_hasher();
    hasher.write_u64(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos() as u64),
    );
    hasher.finish().to_be_bytes()
}

/// `POST /api/videos`: the form fields `participant_code`, `visit_date` and `hand` must come
/// before `file`, so a bad form is refused before any video bytes are read.
pub async fn upload(
    State(state): State<VideoState>,
    mut form: Multipart,
) -> Result<Response, ServerError> {
    let mut code = None;
    let mut date = None;
    let mut hand = None;
    let mut guard = TempGuard(None);
    let mut received = None;
    while let Some(field) = form
        .next_field()
        .await
        .map_err(|_| ServerError::InvalidRequest("The upload form is malformed.".into()))?
    {
        match field.name().unwrap_or("") {
            "participant_code" => code = Some(field_text(field).await?),
            "visit_date" => date = Some(field_text(field).await?),
            "hand" => hand = Some(parse_hand(&field_text(field).await?)?),
            "file" => {
                let meta = UploadMeta {
                    participant_code: code.clone().unwrap_or_default(),
                    visit_date: date.clone().unwrap_or_default(),
                    hand: hand.ok_or_else(|| {
                        ServerError::Field(ApiError::invalid("hand", "Choose left or right."))
                    })?,
                };
                meta.validated().map_err(ServerError::Field)?;
                received = Some(
                    receive_file(field, &state.temp_dir, state.max_upload_bytes, &mut guard)
                        .await?,
                );
                break;
            }
            _ => {
                return Err(ServerError::InvalidRequest("Unexpected form field.".into()));
            }
        }
    }
    let Some(received) = received else {
        return Err(ServerError::Field(ApiError::invalid(
            "file",
            "Choose a video file.",
        )));
    };
    let meta = UploadMeta {
        participant_code: code.unwrap_or_default(),
        visit_date: date.unwrap_or_default(),
        hand: hand.ok_or_else(|| {
            ServerError::Field(ApiError::invalid("hand", "Choose left or right."))
        })?,
    }
    .validated()
    .map_err(ServerError::Field)?;

    let (video, duplicate) =
        blocking(move || finish_upload(&state, received, meta, &mut guard)).await?;
    let status = if duplicate {
        StatusCode::OK
    } else {
        StatusCode::CREATED
    };
    Ok((status, Json(UploadResponse { video, duplicate })).into_response())
}

/// Probe, move into place and index a received file. Returns the record and whether the same
/// bytes were already stored.
fn finish_upload(
    state: &VideoState,
    received: Received,
    meta: UploadMeta,
    guard: &mut TempGuard,
) -> Result<(Video, bool), ServerError> {
    if let Some(existing) = state.store.get(&received.id)? {
        return Ok((existing.video, true));
    }
    let facts = (state.prober)(&received.path).map_err(|failure| match failure {
        ProbeFailure::NotAVideo(message) => ServerError::UnsupportedMedia(format!(
            "This file could not be read as a video: {message}."
        )),
        ProbeFailure::Broken(detail) => ServerError::Internal(detail),
    })?;
    let extension = received.container.extension();
    let final_path = state.video_dir.join(format!("{}.{extension}", received.id));
    std::fs::rename(&received.path, &final_path)
        .map_err(|e| ServerError::Internal(format!("cannot store video: {e}")))?;
    guard.disarm();
    let video = Video {
        id: received.id,
        filename: received.filename,
        participant_code: meta.participant_code,
        visit_date: meta.visit_date,
        hand: meta.hand,
        uploaded_at: now_utc(),
        size_bytes: received.size,
        width: facts.width,
        height: facts.height,
        fps: facts.fps,
        duration_seconds: facts.duration_seconds,
        frame_count: facts.frame_count,
        codec: facts.codec,
        gate: gate::evaluate(facts.fps, facts.duration_seconds),
    };
    match state.store.insert(&video, extension) {
        Ok(true) => Ok((video, false)),
        Ok(false) => {
            // Another upload of the same bytes finished first; its record stands.
            state
                .store
                .get(&video.id)?
                .map(|stored| (stored.video, true))
                .ok_or_else(|| ServerError::Internal("record vanished after insert".into()))
        }
        Err(error) => {
            let _ = std::fs::remove_file(&final_path);
            Err(error.into())
        }
    }
}

/// Query string of `GET /api/videos`.
#[derive(Debug, Deserialize)]
pub struct ListQuery {
    participant_code: Option<String>,
    hand: Option<Hand>,
}

/// `GET /api/videos`
pub async fn list(
    State(state): State<VideoState>,
    Query(query): Query<ListQuery>,
) -> Result<Json<VideoList>, ServerError> {
    let filter = VideoFilter {
        participant_code: query.participant_code.filter(|c| !c.is_empty()),
        hand: query.hand,
    };
    let videos = blocking(move || Ok(state.store.list(&filter)?)).await?;
    Ok(Json(VideoList { videos }))
}

fn not_found() -> ServerError {
    ServerError::NotFound("No such video.".into())
}

/// `GET /api/videos/{id}`
pub async fn get_one(
    State(state): State<VideoState>,
    UrlPath(id): UrlPath<String>,
) -> Result<Json<Video>, ServerError> {
    if !is_video_id(&id) {
        return Err(not_found());
    }
    let stored = blocking(move || Ok(state.store.get(&id)?)).await?;
    stored.map(|s| Json(s.video)).ok_or_else(not_found)
}

/// `GET /api/videos/{id}/stream`: the video bytes, with byte-range support so the browser can
/// seek.
pub async fn stream(
    State(state): State<VideoState>,
    UrlPath(id): UrlPath<String>,
    request: Request<Body>,
) -> Result<Response, ServerError> {
    if !is_video_id(&id) {
        return Err(not_found());
    }
    let dir = state.video_dir.clone();
    let stored = blocking(move || Ok(state.store.get(&id)?))
        .await?
        .ok_or_else(not_found)?;
    let path = dir.join(format!("{}.{}", stored.video.id, stored.extension));
    let response = ServeFile::new(path)
        .oneshot(request)
        .await
        .map_err(|e| ServerError::Internal(format!("cannot stream video: {e}")))?;
    Ok(response.into_response())
}
