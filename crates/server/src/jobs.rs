//! Running tests on videos: the job queue, its worker, and the pipeline for one job.
//!
//! A job goes through two steps. The hand model finds the hand landmarks in the video, then the
//! test plug-in measures them. Jobs run one at a time in the order they were made. Everything the
//! job needs to be repeatable is in its result key, so asking again for the same analysis returns
//! the saved result instead of repeating the work.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axonhand_api::{
    AnalysisResult, ApiError, ComputeDevice, Job, JobRequest, JobState, MeasureValue, ModelInfo,
    QualityCheck, TestDetail,
};
use sha2::{Digest, Sha256};
use tokio::sync::{broadcast, Notify};

use crate::clock::now_utc;
use crate::error::ServerError;
use crate::jobstore::JobRecord;
use crate::media::{hex, is_video_id};
use crate::protocol::{check_requested_metrics, complete_parameters, JobFile};
use crate::registry::{Plugin, Registry};
use crate::result_key::{result_key, KeyInputs};
use crate::results::{rebuild_index, ResultFiles};
use crate::runner::{run, Limits, RunError};
use crate::rust_hand::rust_extractor;
use crate::store::Store;

/// Why the hand model could not process a video.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtractFailure {
    /// The video cannot be used, or the request cannot be run as asked (for example the NPU was
    /// chosen and there is none). The text is safe to show to the user.
    NotUsable(String),
    /// The model could not run. The text is for the log only.
    Broken(String),
}

/// Finds hand landmarks in a video on the given device. Blocking; runs inside `spawn_blocking`.
pub type ExtractFn =
    Arc<dyn Fn(&Path, &Path, ComputeDevice) -> Result<(), ExtractFailure> + Send + Sync>;

/// The hand model: who it is, and how to run it.
#[derive(Clone)]
pub struct Extractor {
    /// Identity of the model's files and pipeline. Use [`Extractor::identity`] for the identity
    /// of a result, which also says where the model ran.
    pub model: ModelInfo,
    /// Runs the model on `(video, landmarks_output_path, device)`.
    pub run: ExtractFn,
    /// True once this model's accuracy has been measured on annotated recordings. Results from a
    /// model that is not validated are always labelled illustrative.
    pub validated: bool,
}

impl Extractor {
    /// The identity recorded with a result made on `device`.
    ///
    /// The CPU and the NPU give slightly different landmarks, so each has its own hash and so its
    /// own result key. A stand-in does not depend on the device and keeps one identity.
    #[must_use]
    pub fn identity(&self, device: ComputeDevice) -> ModelInfo {
        if self.model.stand_in {
            return self.model.clone();
        }
        ModelInfo {
            hash: hex(&Sha256::digest(
                format!("{}\ndevice:{}", self.model.hash, device.flag()).as_bytes(),
            )),
            device: device.label().to_owned(),
            ..self.model.clone()
        }
    }
}

/// The longest the hand model may take on one video.
pub(crate) const EXTRACT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20 * 60);

/// The command `python -m <module> VIDEO OUTPUT [extra...]`.
fn hand_tool_command(
    python: &Path,
    module: &str,
    extra: &[&std::ffi::OsStr],
    video: &Path,
    output: &Path,
) -> Command {
    let mut command = Command::new(python);
    command
        .args(["-m", module])
        .arg(video)
        .arg(output)
        .args(extra);
    command
}

/// Marks the line of a tool's error output that is meant for the person (the model runtime may
/// write warnings of its own to the same stream).
const MESSAGE_MARKER: &str = "axonhand-message: ";

/// The sentence for the person from a tool's error output: the line that starts with the marker,
/// or, for a tool without markers, its last non-empty line.
pub(crate) fn user_message(stderr: &str) -> String {
    let line = stderr
        .lines()
        .rev()
        .find_map(|l| l.strip_prefix(MESSAGE_MARKER))
        .or_else(|| stderr.lines().rev().find(|l| !l.trim().is_empty()))
        .unwrap_or("");
    line.trim().chars().take(300).collect()
}

/// Run a hand tool and read its exit status.
///
/// Exit status 0 is success, 2 means the video cannot be used (the tool's message for the person
/// is on stderr), anything else is a fault. The tool's output is kept next to the landmarks as
/// `*.out.log` and `*.err.log`, so a failure can be examined afterwards.
fn run_hand_tool(mut command: Command, output: &Path) -> Result<(), ExtractFailure> {
    use std::process::Stdio;
    let broken =
        |what: &str, e: &dyn std::fmt::Display| ExtractFailure::Broken(format!("{what}: {e}"));
    let out_path = output.with_extension("out.log");
    let err_path = output.with_extension("err.log");
    let stdout = std::fs::File::create(&out_path).map_err(|e| broken("log file", &e))?;
    let stderr = std::fs::File::create(&err_path).map_err(|e| broken("log file", &e))?;
    let mut child = command
        .env("PYTHONUTF8", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .map_err(|e| broken("cannot start the hand model", &e))?;
    let started = std::time::Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(e) => {
                let _ = child.kill();
                return Err(broken("waiting for the hand model", &e));
            }
        }
        if started.elapsed() >= EXTRACT_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            return Err(ExtractFailure::Broken(
                "the hand model took too long".to_owned(),
            ));
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    let text = |path: &Path| {
        std::fs::read_to_string(path)
            .unwrap_or_default()
            .trim()
            .to_owned()
    };
    match status.code() {
        Some(0) => {
            tracing::info!(summary = %text(&out_path), "hand model finished");
            Ok(())
        }
        Some(2) => Err(ExtractFailure::NotUsable(user_message(&text(&err_path)))),
        other => Err(ExtractFailure::Broken(format!(
            "exit status {other:?}: {}",
            text(&err_path)
        ))),
    }
}

/// The development stand-in: runs `python -m axonhand.fake_model`.
#[must_use]
pub fn stand_in_extractor(python: PathBuf) -> Extractor {
    let run: ExtractFn = Arc::new(move |video, output, _device| {
        run_hand_tool(
            hand_tool_command(&python, "axonhand.fake_model", &[], video, output),
            output,
        )
    });
    Extractor {
        model: ModelInfo {
            name: "Stand-in colour tracker".to_owned(),
            hash: hex(&Sha256::digest(b"axonhand-stand-in-colour-tracker-v1")),
            stand_in: true,
            device: String::new(),
        },
        run,
        validated: false,
    }
}

/// Names of the two ONNX files in the models folder, and the version of the pipeline that uses
/// them. Changing any of them changes the model hash, so old results are never mistaken for new.
pub(crate) const PALM_MODEL: &str = "palm_detection_mediapipe_2023feb.onnx";
pub(crate) const LANDMARK_MODEL: &str = "handpose_estimation_mediapipe_2023feb.onnx";
pub(crate) const PIPELINE_VERSION: &str = "mediapipe-two-stage-1";

/// Fingerprint of a pipeline version and the two model files, so that changing any of them changes
/// the model's hash and old results are never mistaken for new.
///
/// # Errors
///
/// A sentence naming the file that cannot be read.
pub(crate) fn models_fingerprint(models_dir: &Path, version: &str) -> Result<String, String> {
    let mut hasher = Sha256::new();
    hasher.update(version.as_bytes());
    for name in [PALM_MODEL, LANDMARK_MODEL] {
        let path = models_dir.join(name);
        let bytes =
            std::fs::read(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        // Each file is hashed on its own so that two files cannot be swapped unnoticed.
        hasher.update(Sha256::digest(&bytes));
    }
    Ok(hex(&hasher.finalize()))
}

/// The real hand model: runs `python -m axonhand.hand_model` with the ONNX files in `models_dir`.
///
/// # Errors
///
/// A sentence naming the file that is missing or cannot be read.
pub fn mediapipe_extractor(python: PathBuf, models_dir: &Path) -> Result<Extractor, String> {
    let models_dir = std::path::absolute(models_dir)
        .map_err(|e| format!("cannot use the models folder {}: {e}", models_dir.display()))?;
    let hash = models_fingerprint(&models_dir, PIPELINE_VERSION)?;
    let run: ExtractFn = Arc::new(move |video, output, device| {
        run_hand_tool(
            hand_tool_command(
                &python,
                "axonhand.hand_model",
                &[
                    "--models".as_ref(),
                    models_dir.as_os_str(),
                    "--device".as_ref(),
                    device.flag().as_ref(),
                ],
                video,
                output,
            ),
            output,
        )
    });
    Ok(Extractor {
        model: ModelInfo {
            name: "MediaPipe hand landmarks (ONNX)".to_owned(),
            hash,
            stand_in: false,
            device: String::new(),
        },
        run,
        validated: false,
    })
}

/// Which hand model to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandModelChoice {
    /// The best real model that can be used here (the Rust tracker, otherwise the Python
    /// pipeline), or the stand-in if the model files are missing.
    Auto,
    /// Always the development stand-in.
    StandIn,
    /// Always the real model, run by Python; starting fails if its files are missing.
    Mediapipe,
    /// Always the real model, run by the Rust tracker (on the NPU when there is one); starting
    /// fails if its files or the tracker program are missing.
    MediapipeRust,
}

/// Pick the hand model as the settings say.
///
/// # Errors
///
/// A sentence explaining why `Mediapipe` was demanded but cannot be used.
pub fn choose_extractor(
    choice: HandModelChoice,
    python: &Path,
    models_dir: &Path,
    track_binary: &Path,
) -> Result<Extractor, String> {
    match choice {
        HandModelChoice::StandIn => Ok(stand_in_extractor(python.to_path_buf())),
        HandModelChoice::Mediapipe => mediapipe_extractor(python.to_path_buf(), models_dir),
        HandModelChoice::MediapipeRust => {
            rust_extractor(python.to_path_buf(), models_dir, track_binary)
        }
        HandModelChoice::Auto => Ok(auto_extractor(python, models_dir, track_binary)),
    }
}

/// The best hand model that can be used here: the Rust tracker, then the Python pipeline with the
/// same models, then the development stand-in. Each step down is logged with its reason, so a
/// computer that quietly uses a slower or lesser model can be explained.
fn auto_extractor(python: &Path, models_dir: &Path, track_binary: &Path) -> Extractor {
    match rust_extractor(python.to_path_buf(), models_dir, track_binary) {
        Ok(extractor) => return extractor,
        Err(reason) => {
            tracing::info!(%reason, "not using the Rust hand tracker; trying the Python pipeline");
        }
    }
    match mediapipe_extractor(python.to_path_buf(), models_dir) {
        Ok(extractor) => extractor,
        Err(reason) => {
            tracing::warn!(%reason, "using the stand-in hand model; run scripts/fetch_models.ps1");
            stand_in_extractor(python.to_path_buf())
        }
    }
}

/// Everything a job needs.
pub struct JobContext {
    /// The index.
    pub store: Arc<Store>,
    /// Saved results.
    pub results: ResultFiles,
    /// Installed tests.
    pub registry: Arc<Registry>,
    /// The hand model.
    pub extractor: Extractor,
    /// Python interpreter for plug-ins.
    pub python: PathBuf,
    /// Folder of stored videos.
    pub video_dir: PathBuf,
    /// Folder for per-job scratch files.
    pub work_dir: PathBuf,
    /// Limits for plug-in runs.
    pub limits: Limits,
}

/// Creates jobs and runs them one at a time.
pub struct JobManager {
    ctx: JobContext,
    events: broadcast::Sender<Job>,
    wake: Notify,
}

fn new_id() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let mut hasher = RandomState::new().build_hasher();
    hasher.write_u128(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos()),
    );
    format!("{:016x}", hasher.finish())
}

fn internal(error: impl std::fmt::Display) -> ServerError {
    ServerError::Internal(error.to_string())
}

impl JobManager {
    /// Create the manager. Call [`JobManager::start`] to begin processing.
    #[must_use]
    pub fn new(ctx: JobContext) -> Arc<Self> {
        let (events, _) = broadcast::channel(256);
        Arc::new(Self {
            ctx,
            events,
            wake: Notify::new(),
        })
    }

    /// Put interrupted jobs back in the queue, index results found on disk, and start the
    /// worker on the current Tokio runtime.
    ///
    /// # Errors
    ///
    /// A sentence describing what could not be set up.
    pub fn start(self: &Arc<Self>) -> Result<tokio::task::JoinHandle<()>, String> {
        let requeued = self
            .ctx
            .store
            .requeue_running_jobs()
            .map_err(|e| e.to_string())?;
        if requeued > 0 {
            tracing::info!(requeued, "jobs interrupted by a restart were queued again");
        }
        let indexed =
            rebuild_index(&self.ctx.results, &self.ctx.store).map_err(|e| e.to_string())?;
        if indexed > 0 {
            tracing::info!(indexed, "results found on disk were added to the index");
        }
        let manager = Arc::clone(self);
        Ok(tokio::spawn(async move { manager.worker().await }))
    }

    /// Follow changes to jobs. Every state or progress change is sent once.
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<Job> {
        self.events.subscribe()
    }

    /// Current state of a job.
    ///
    /// # Errors
    ///
    /// [`ServerError`] on a database failure.
    pub fn job(&self, id: &str) -> Result<Option<Job>, ServerError> {
        Ok(self.ctx.store.get_job(id).map_err(internal)?.map(|r| r.job))
    }

    /// Saved results, newest first, optionally only of one test or one participant.
    ///
    /// # Errors
    ///
    /// [`ServerError`] on a database failure.
    pub fn list_results(
        &self,
        test_id: Option<&str>,
        participant_code: Option<&str>,
    ) -> Result<Vec<axonhand_api::ResultSummary>, ServerError> {
        self.ctx
            .store
            .list_results(test_id, participant_code)
            .map_err(internal)
    }

    /// Whether a result with this key has been saved.
    ///
    /// # Errors
    ///
    /// [`ServerError`] on a database failure.
    pub fn result_exists(&self, key: &str) -> Result<bool, ServerError> {
        self.ctx.store.result_indexed(key).map_err(internal)
    }

    /// Save a clinician's score for a result, replacing that rater's earlier one.
    ///
    /// # Errors
    ///
    /// [`ServerError`] on a database failure.
    pub fn put_score(
        &self,
        key: &str,
        request: &axonhand_api::ScoreRequest,
    ) -> Result<axonhand_api::ClinicianScore, ServerError> {
        self.ctx
            .store
            .put_score(key, request, &now_utc())
            .map_err(internal)
    }

    /// The scores clinicians gave to a result.
    ///
    /// # Errors
    ///
    /// [`ServerError`] on a database failure.
    pub fn scores(&self, key: &str) -> Result<Vec<axonhand_api::ClinicianScore>, ServerError> {
        self.ctx.store.scores(key).map_err(internal)
    }

    /// The saved results.
    #[must_use]
    pub fn results(&self) -> &ResultFiles {
        &self.ctx.results
    }

    /// Check a request and create a job for it. The flag is `true` when new work was queued and
    /// `false` when the answer already existed or the same work is already waiting or running.
    ///
    /// # Errors
    ///
    /// [`ServerError`] naming the field that is wrong, `not_found` for an unknown video.
    pub fn submit(&self, request: &JobRequest) -> Result<(Job, bool), ServerError> {
        let field =
            |name: &str, message: &str| ServerError::Field(ApiError::invalid(name, message));
        if !is_video_id(&request.video_id)
            || self
                .ctx
                .store
                .get(&request.video_id)
                .map_err(internal)?
                .is_none()
        {
            return Err(ServerError::NotFound("No such video.".to_owned()));
        }
        let plugin = self
            .ctx
            .registry
            .runnable(&request.test_id)
            .ok_or_else(|| {
                field(
                    "test_id",
                    "This test is not installed or cannot be run yet.",
                )
            })?;
        check_requested_metrics(&plugin.detail, &request.metrics)
            .map_err(|r| field("metrics", &r.0))?;
        let parameters = complete_parameters(&plugin.detail, &request.parameters)
            .map_err(|r| field("parameters", &r.0))?;
        let key = result_key(&KeyInputs {
            video_id: &request.video_id,
            test_id: &request.test_id,
            test_version: &plugin.detail.summary.version,
            model_hash: &self.ctx.extractor.identity(request.device).hash,
            parameters: &parameters,
            metrics: &request.metrics,
        });
        if let Some(active) = self.ctx.store.find_active_job(&key).map_err(internal)? {
            return Ok((active.job, false));
        }
        let finished = self.ctx.results.exists(&key).map_err(internal)?;
        let job = Job {
            id: new_id(),
            state: if finished {
                JobState::Succeeded
            } else {
                JobState::Queued
            },
            result_key: key,
            video_id: request.video_id.clone(),
            test_id: request.test_id.clone(),
            step: String::new(),
            progress: if finished { 1.0 } else { 0.0 },
            error: None,
            created_at: now_utc(),
        };
        let stored = JobRequest {
            parameters,
            ..request.clone()
        };
        self.ctx.store.insert_job(&job, &stored).map_err(internal)?;
        self.publish(&job);
        if !finished {
            self.wake.notify_one();
        }
        Ok((job, !finished))
    }

    fn publish(&self, job: &Job) {
        // No subscribers is the normal case; there is nobody to tell.
        let _ = self.events.send(job.clone());
    }

    fn report(&self, id: &str, step: &str, progress: f64) {
        if self.ctx.store.set_job_progress(id, step, progress).is_ok() {
            if let Ok(Some(job)) = self.job(id) {
                self.publish(&job);
            }
        }
    }

    async fn worker(self: Arc<Self>) {
        loop {
            let manager = Arc::clone(&self);
            let claimed =
                tokio::task::spawn_blocking(move || manager.ctx.store.claim_next_job()).await;
            match claimed {
                Ok(Ok(Some(record))) => {
                    self.publish(&record.job);
                    let manager = Arc::clone(&self);
                    if let Err(error) =
                        tokio::task::spawn_blocking(move || manager.process(&record)).await
                    {
                        tracing::error!(%error, "a job's worker thread failed");
                    }
                }
                Ok(Ok(None)) => self.wake.notified().await,
                Ok(Err(error)) => {
                    tracing::error!(%error, "cannot read the job queue");
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
                Err(error) => {
                    tracing::error!(%error, "the job queue task failed");
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
            }
        }
    }

    /// Run one claimed job to the end and record the outcome.
    fn process(&self, record: &JobRecord) {
        let id = &record.job.id;
        let outcome = self.execute(record);
        let error = outcome.err();
        if let Some(message) = &error {
            tracing::warn!(job = %id, error = %message, "job failed");
        }
        if let Err(e) = self.ctx.store.finish_job(id, error.as_deref()) {
            tracing::error!(job = %id, error = %e, "cannot record the end of a job");
        }
        if let Ok(Some(job)) = self.job(id) {
            self.publish(&job);
        }
        // Scratch files are not needed once the result is saved. After a failure they are kept,
        // because the plug-in's `output.log` there is the best clue to what went wrong.
        if error.is_none() {
            let _ = std::fs::remove_dir_all(self.ctx.work_dir.join(id));
        }
    }

    /// The pipeline. `Err` carries a sentence safe to show to the user.
    fn execute(&self, record: &JobRecord) -> Result<(), String> {
        const GENERIC: &str = "The analysis could not be completed because of a server problem.";
        let job = &record.job;
        let request = &record.request;
        let log = |what: &str, error: &dyn std::fmt::Display| {
            tracing::error!(job = %job.id, %what, %error, "job step failed");
            GENERIC.to_owned()
        };
        if self
            .ctx
            .results
            .exists(&job.result_key)
            .map_err(|e| log("check result", &e))?
        {
            return Ok(());
        }
        let plugin = self
            .ctx
            .registry
            .runnable(&job.test_id)
            .ok_or_else(|| "This test is no longer installed.".to_owned())?;
        let stored = self
            .ctx
            .store
            .get(&job.video_id)
            .map_err(|e| log("read video", &e))?
            .ok_or_else(|| "The video is no longer stored.".to_owned())?;
        let video = crate::runner::absolute(
            &self
                .ctx
                .video_dir
                .join(format!("{}.{}", stored.video.id, stored.extension)),
        )
        .map_err(|e| log("video path", &e))?;
        let work = self.ctx.work_dir.join(&job.id);
        std::fs::create_dir_all(&work).map_err(|e| log("work folder", &e))?;

        self.report(&job.id, "Finding the hand", 0.05);
        let work = crate::runner::absolute(&work).map_err(|e| log("work folder", &e))?;
        let landmarks = work.join("landmarks.npz");
        (self.ctx.extractor.run)(&video, &landmarks, request.device).map_err(|failure| {
            match failure {
                ExtractFailure::NotUsable(message) if !message.is_empty() => message,
                ExtractFailure::NotUsable(_) => {
                    "The hand could not be found in this video.".to_owned()
                }
                ExtractFailure::Broken(detail) => log("hand model", &detail),
            }
        })?;

        self.report(&job.id, "Measuring", 0.6);
        let file = JobFile {
            schema_version: crate::protocol::SCHEMA_VERSION,
            job_id: job.id.clone(),
            test_id: job.test_id.clone(),
            video_path: video.display().to_string(),
            landmarks_path: Some(landmarks.display().to_string()),
            metrics: request.metrics.clone(),
            parameters: request.parameters.clone(),
        };
        let output = run(
            &self.ctx.python,
            plugin,
            &file,
            &work.join("plugin"),
            self.ctx.limits,
        )
        .map_err(|error| match error {
            RunError::Launch(detail) => log("plug-in launch", &detail),
            other => other.to_string(),
        })?;

        self.report(&job.id, "Saving", 0.9);
        // The landmarks to draw over the video are optional: a tool that does not write them
        // (or a file that cannot be read) only means no overlay, never a failed analysis.
        let overlay = std::fs::read(landmarks.with_extension("json")).ok();
        let result = self.build_result(job, request, plugin, &output.result);
        self.ctx
            .results
            .write(
                &job.result_key,
                &result,
                output.signals.as_ref(),
                overlay.as_deref(),
            )
            .map_err(|e| log("save result", &e))?;
        self.ctx
            .store
            .index_result(
                &job.result_key,
                &job.video_id,
                &job.test_id,
                &plugin.detail.summary.version,
                &result.created_at,
            )
            .map_err(|e| log("index result", &e))?;
        Ok(())
    }

    fn build_result(
        &self,
        job: &Job,
        request: &JobRequest,
        plugin: &Plugin,
        output: &crate::protocol::PluginResult,
    ) -> AnalysisResult {
        let detail: &TestDetail = &plugin.detail;
        let measures = request
            .metrics
            .iter()
            .filter_map(|id| {
                let definition = detail.metrics.iter().find(|m| &m.id == id)?;
                let measure = output.measures.iter().find(|m| &m.metric_id == id)?;
                Some(MeasureValue {
                    metric_id: id.clone(),
                    name: definition.name.clone(),
                    unit: definition.unit.clone(),
                    value: measure.value,
                    reason: measure.reason.clone(),
                })
            })
            .collect();
        let model = self.ctx.extractor.identity(request.device);
        let illustrative = model.stand_in || !self.ctx.extractor.validated;
        let mut notes = output.notes.clone();
        if model.stand_in {
            notes.push(
                "These numbers come from a stand-in tracker used during development. They are \
                 illustrative and are not measurements of a real hand."
                    .to_owned(),
            );
        } else if illustrative {
            notes.push(
                "These numbers come from the hand model, but its accuracy on clinical recordings \
                 has not been measured yet, so they are illustrative."
                    .to_owned(),
            );
        }
        let quality: Vec<QualityCheck> = output.quality.clone();
        AnalysisResult {
            key: job.result_key.clone(),
            video_id: job.video_id.clone(),
            test_id: job.test_id.clone(),
            test_name: detail.summary.name.clone(),
            test_version: detail.summary.version.clone(),
            illustrative,
            model,
            parameters: request.parameters.clone(),
            measures,
            quality,
            notes,
            created_at: now_utc(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn models(dir: &Path, palm: &[u8], landmarks: &[u8]) -> std::io::Result<()> {
        std::fs::write(dir.join(PALM_MODEL), palm)?;
        std::fs::write(dir.join(LANDMARK_MODEL), landmarks)
    }

    #[test]
    fn the_real_model_is_chosen_when_its_files_are_present(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        models(dir.path(), b"a", b"b")?;
        let e = choose_extractor(
            HandModelChoice::Auto,
            Path::new("python"),
            dir.path(),
            Path::new("axonhand-track"),
        )?;
        assert!(!e.model.stand_in && !e.validated);
        assert_eq!(e.model.hash.len(), 64);
        Ok(())
    }

    #[test]
    fn auto_uses_the_python_pipeline_when_the_rust_tracker_cannot_be_used(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        models(dir.path(), b"a", b"b")?;
        let e = choose_extractor(
            HandModelChoice::Auto,
            Path::new("python"),
            dir.path(),
            Path::new("no-such-tracker-program"),
        )?;
        assert!(!e.model.stand_in);
        assert!(!e.model.name.contains("Rust"), "{}", e.model.name);
        Ok(())
    }

    #[test]
    fn auto_falls_back_to_the_stand_in_without_the_files() -> Result<(), Box<dyn std::error::Error>>
    {
        let dir = tempfile::tempdir()?;
        let e = choose_extractor(
            HandModelChoice::Auto,
            Path::new("python"),
            dir.path(),
            Path::new("axonhand-track"),
        )?;
        assert!(e.model.stand_in);
        Ok(())
    }

    #[test]
    fn demanding_the_real_model_without_its_files_is_an_error_naming_the_file(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let Err(message) = choose_extractor(
            HandModelChoice::Mediapipe,
            Path::new("python"),
            dir.path(),
            Path::new("axonhand-track"),
        ) else {
            return Err("expected an error".into());
        };
        assert!(message.contains(PALM_MODEL), "{message}");
        Ok(())
    }

    #[test]
    fn the_rust_tracker_names_what_is_missing() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let ask = |models: &Path, tracker: &str| {
            choose_extractor(
                HandModelChoice::MediapipeRust,
                Path::new("python"),
                models,
                Path::new(tracker),
            )
            .err()
            .unwrap_or_default()
        };
        assert!(ask(dir.path(), "axonhand-track").contains(PALM_MODEL));
        models(dir.path(), b"a", b"b")?;
        let message = ask(dir.path(), "no-such-tracker-program");
        assert!(message.contains("hand tracker"), "{message}");
        Ok(())
    }

    #[test]
    fn the_stand_in_can_be_forced_even_when_the_files_exist(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        models(dir.path(), b"a", b"b")?;
        let e = choose_extractor(
            HandModelChoice::StandIn,
            Path::new("python"),
            dir.path(),
            Path::new("axonhand-track"),
        )?;
        assert!(e.model.stand_in);
        Ok(())
    }

    #[test]
    fn the_cpu_and_the_npu_have_different_identities_but_a_stand_in_has_one() -> Result<(), String>
    {
        let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
        for name in [PALM_MODEL, LANDMARK_MODEL] {
            std::fs::write(dir.path().join(name), name).map_err(|e| e.to_string())?;
        }
        let real = mediapipe_extractor(PathBuf::from("python"), dir.path())?;
        let (cpu, npu) = (
            real.identity(ComputeDevice::Cpu),
            real.identity(ComputeDevice::Npu),
        );
        assert_ne!(cpu.hash, npu.hash);
        assert_eq!((cpu.device.as_str(), npu.device.as_str()), ("CPU", "NPU"));
        assert_eq!(cpu.name, npu.name);
        let stand_in = stand_in_extractor(PathBuf::from("python"));
        assert_eq!(
            stand_in.identity(ComputeDevice::Cpu),
            stand_in.identity(ComputeDevice::Npu)
        );
        Ok(())
    }

    #[test]
    fn changing_either_file_or_swapping_them_changes_the_model_hash(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let hash = |palm: &[u8], landmarks: &[u8]| -> Result<String, Box<dyn std::error::Error>> {
            let dir = tempfile::tempdir()?;
            models(dir.path(), palm, landmarks)?;
            Ok(mediapipe_extractor(PathBuf::from("python"), dir.path())?
                .model
                .hash)
        };
        let base = hash(b"palm", b"landmarks")?;
        assert_eq!(base, hash(b"palm", b"landmarks")?);
        assert_ne!(base, hash(b"palm!", b"landmarks")?);
        assert_ne!(base, hash(b"palm", b"landmarks!")?);
        assert_ne!(base, hash(b"landmarks", b"palm")?);
        Ok(())
    }

    fn python() -> PathBuf {
        std::env::var_os("AXONHAND_PYTHON").map_or_else(
            || PathBuf::from(if cfg!(windows) { "python" } else { "python3" }),
            PathBuf::from,
        )
    }

    /// A hand tool that is a tiny module which exits with `code` after printing `message`.
    fn tool(
        code: i32,
        message: &str,
    ) -> Result<(tempfile::TempDir, Command, PathBuf), std::io::Error> {
        let dir = tempfile::tempdir()?;
        std::fs::write(
            dir.path().join("fake_tool.py"),
            format!("import sys\nprint({message:?}, file=sys.stderr)\nsys.exit({code})\n"),
        )?;
        let mut command = Command::new(python());
        command.current_dir(dir.path()).args(["-m", "fake_tool"]);
        let out = dir.path().join("o.npz");
        Ok((dir, command, out))
    }

    #[test]
    fn a_tool_that_exits_with_2_gives_its_message_as_not_usable(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let (_dir, command, out) = tool(2, "No hand was found.")?;
        assert_eq!(
            run_hand_tool(command, &out),
            Err(ExtractFailure::NotUsable("No hand was found.".to_owned()))
        );
        Ok(())
    }

    #[test]
    fn only_the_marked_line_is_shown_to_the_person() {
        let noisy = "2026 [W:onnxruntime:, ep_context_io_dispatch.cc:29] EPContext\n\
                     axonhand-message: No hand was found in this video.\n";
        assert_eq!(user_message(noisy), "No hand was found in this video.");
        // A tool without markers: its last line.
        assert_eq!(
            user_message("warning\nThe video could not be read.\n"),
            "The video could not be read."
        );
        assert_eq!(user_message(""), "");
        let long = format!("{MESSAGE_MARKER}{}", "x".repeat(1000));
        assert_eq!(user_message(&long).len(), 300);
    }

    #[test]
    fn a_tool_that_exits_with_any_other_status_is_a_fault() -> Result<(), Box<dyn std::error::Error>>
    {
        let (_dir, command, out) = tool(1, "boom")?;
        let Err(ExtractFailure::Broken(detail)) = run_hand_tool(command, &out) else {
            return Err("expected a fault".into());
        };
        assert!(detail.contains("boom"), "{detail}");
        Ok(())
    }

    #[test]
    fn a_tool_that_succeeds_is_ok_and_a_missing_python_is_a_fault(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let (_dir, command, out) = tool(0, "fine")?;
        assert_eq!(run_hand_tool(command, &out), Ok(()));
        let dir = tempfile::tempdir()?;
        let missing = Command::new(dir.path().join("no-such-python"));
        assert!(matches!(
            run_hand_tool(missing, &dir.path().join("o.npz")),
            Err(ExtractFailure::Broken(_))
        ));
        Ok(())
    }
}
