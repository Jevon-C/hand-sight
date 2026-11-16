//! Jobs, results and the signals behind their plots.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::Hand;

/// Where a job stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    /// Waiting for its turn. Jobs run one at a time.
    Queued,
    /// Being worked on.
    Running,
    /// Finished; the result can be read.
    Succeeded,
    /// Stopped with an error; see the job's `error`.
    Failed,
}

/// Where the hand model runs.
///
/// The two give slightly different landmarks (the NPU computes with less precision), so they are
/// separate results: the choice is part of the result key and is recorded with the result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ComputeDevice {
    /// The processor. Exact and the same on every computer. The default.
    #[default]
    Cpu,
    /// The neural processing unit: faster, with slightly different landmarks.
    Npu,
}

impl ComputeDevice {
    /// The word the hand tools take after `--device`.
    #[must_use]
    pub fn flag(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Npu => "npu",
        }
    }

    /// The name to show to a person.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Cpu => "CPU",
            Self::Npu => "NPU",
        }
    }
}

/// Body of `POST /api/jobs`: which video to analyse with which test.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct JobRequest {
    /// Id of a stored video.
    pub video_id: String,
    /// Id of an available test.
    pub test_id: String,
    /// Metric ids to compute. At least one.
    pub metrics: Vec<String>,
    /// Settings to change from their defaults.
    #[serde(default)]
    pub parameters: BTreeMap<String, f64>,
    /// Where to run the hand model. The CPU unless the NPU is asked for.
    #[serde(default)]
    pub device: ComputeDevice,
}

/// A request to analyse a video, and how far it has got.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Job {
    /// Identifier of the job.
    pub id: String,
    /// Current state.
    pub state: JobState,
    /// Key of the result this job produces. The same video, test version, model and settings
    /// always give the same key.
    pub result_key: String,
    /// The video being analysed.
    pub video_id: String,
    /// The test being run.
    pub test_id: String,
    /// What is happening now, in plain words. Empty when queued or finished.
    pub step: String,
    /// Fraction of the work done, from 0 to 1.
    pub progress: f64,
    /// Why the job failed, when it did. Safe to show to the user.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// When the job was created, as UTC `YYYY-MM-DDTHH:MM:SSZ`.
    pub created_at: String,
}

/// The hand model that produced the landmarks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ModelInfo {
    /// Name of the model.
    pub name: String,
    /// Fingerprint of the model's files, so a different model gives a different result key.
    pub hash: String,
    /// True for a stand-in used in development. Its numbers are not measurements of a real hand.
    pub stand_in: bool,
    /// Where the model ran: `CPU` or `NPU`. Empty for a stand-in, and for results saved before
    /// this was recorded.
    #[serde(default)]
    pub device: String,
}

/// One number of a result, with its definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct MeasureValue {
    /// Metric identifier.
    pub metric_id: String,
    /// Name shown to people.
    pub name: String,
    /// Unit of the value.
    pub unit: String,
    /// The value, or `None` when it could not be measured.
    pub value: Option<f64>,
    /// Why there is no value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Outcome of a quality check on a result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum QualityStatus {
    /// Nothing to note.
    Pass,
    /// Read the numbers with care.
    Flag,
    /// The numbers should not be relied on.
    Fail,
}

/// One quality check on a result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct QualityCheck {
    /// What was checked.
    pub name: String,
    /// How it came out.
    pub status: QualityStatus,
    /// A plain-language explanation.
    pub detail: String,
}

/// A saved analysis. Never changed after it is written.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct AnalysisResult {
    /// The result key.
    pub key: String,
    /// The video that was analysed.
    pub video_id: String,
    /// The test that was run.
    pub test_id: String,
    /// Name of the test.
    pub test_name: String,
    /// Version of the test plug-in that produced the numbers.
    pub test_version: String,
    /// The hand model used.
    pub model: ModelInfo,
    /// Every setting the test ran with.
    pub parameters: BTreeMap<String, f64>,
    /// The numbers.
    pub measures: Vec<MeasureValue>,
    /// Checks on how far the numbers can be trusted.
    pub quality: Vec<QualityCheck>,
    /// Remarks from the test and from the system.
    pub notes: Vec<String>,
    /// True when the numbers must be shown with an "Illustrative" label because they do not come
    /// from a validated pipeline.
    pub illustrative: bool,
    /// When the result was created, as UTC `YYYY-MM-DDTHH:MM:SSZ`.
    pub created_at: String,
}

/// A signal sampled at a fixed rate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SeriesData {
    /// Identifier.
    pub id: String,
    /// Name shown to people.
    pub name: String,
    /// Unit of the values.
    pub unit: String,
    /// Time of the first value, in seconds.
    pub start_seconds: f64,
    /// Seconds between values.
    pub step_seconds: f64,
    /// The values.
    pub values: Vec<f64>,
}

/// Marked moments with a value each, for example one per tap.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct EventData {
    /// Identifier.
    pub id: String,
    /// Name shown to people.
    pub name: String,
    /// Unit of the values.
    pub unit: String,
    /// Time of each event, in seconds.
    pub times: Vec<f64>,
    /// Value of each event.
    pub values: Vec<f64>,
}

/// Body of `GET /api/results/{key}/signals`: the data behind a result's plots.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Signals {
    /// Regularly sampled signals.
    pub series: Vec<SeriesData>,
    /// Marked moments.
    pub events: Vec<EventData>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_state_uses_snake_case() -> Result<(), serde_json::Error> {
        assert_eq!(
            serde_json::to_string(&JobState::Succeeded)?,
            "\"succeeded\""
        );
        Ok(())
    }

    #[test]
    fn a_job_request_without_parameters_parses() -> Result<(), serde_json::Error> {
        let request: JobRequest =
            serde_json::from_str(r#"{"video_id":"v","test_id":"t","metrics":["a"]}"#)?;
        assert!(request.parameters.is_empty());
        Ok(())
    }

    #[test]
    fn an_absent_error_is_left_out() -> Result<(), serde_json::Error> {
        let job = Job {
            id: "j".into(),
            state: JobState::Queued,
            result_key: "k".into(),
            video_id: "v".into(),
            test_id: "t".into(),
            step: String::new(),
            progress: 0.0,
            error: None,
            created_at: "2026-01-01T00:00:00Z".into(),
        };
        assert!(!serde_json::to_string(&job)?.contains("error"));
        Ok(())
    }
}

/// The hand landmarks of a video, for drawing over it. Served by `GET /api/results/{key}/landmarks`.
///
/// Positions are in pixels of the upright picture (the way a person sees the video), so they can
/// be scaled to the size the video is shown at.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct LandmarkOverlay {
    /// Frames per second of the video, to turn a playing time into a frame number.
    pub fps: f64,
    /// For each frame, the 21 landmarks as `[x, y]` in the MediaPipe numbering (`None` for one
    /// that was not found), or `None` for the whole frame when no hand was found.
    pub points: Vec<Option<Vec<Option<[f64; 2]>>>>,
}

/// One saved result in a list, with the video it came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ResultSummary {
    /// The result key.
    pub key: String,
    /// The video that was analysed.
    pub video_id: String,
    /// The test that was run.
    pub test_id: String,
    /// Version of the test plug-in that produced the numbers.
    pub test_version: String,
    /// When the result was created, as UTC `YYYY-MM-DDTHH:MM:SSZ`.
    pub created_at: String,
    /// Participant code of the video.
    pub participant_code: String,
    /// Visit date of the video, as `YYYY-MM-DD`.
    pub visit_date: String,
    /// Which hand was filmed.
    pub hand: Hand,
}

/// Body of `GET /api/results`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ResultList {
    /// The matching results, newest first.
    pub results: Vec<ResultSummary>,
}
