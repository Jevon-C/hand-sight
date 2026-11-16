//! Types shared by the Hand Sight server and the browser app.
//!
//! Both sides depend on this crate, so a change to a request or response shape that is not
//! made on both sides fails to compile instead of failing at run time. The crate has no
//! knowledge of HTTP or of the browser; it only describes data.
//!
//! [`openapi_json`] renders the same types as an OpenAPI document, which is checked into
//! `docs/api/openapi.json` and kept current by a test.

#![deny(missing_docs)]

mod analysis;
mod error;
mod health;
mod openapi;
mod scores;
mod tests_registry;
mod video;

pub use analysis::{
    AnalysisResult, ComputeDevice, EventData, Job, JobRequest, JobState, LandmarkOverlay,
    MeasureValue, ModelInfo, QualityCheck, QualityStatus, ResultList, ResultSummary, SeriesData,
    Signals,
};
pub use error::{ApiError, ErrorCode};
pub use health::{Health, HealthStatus};
pub use openapi::openapi_json;
pub use scores::{ClinicianScore, ScoreList, ScoreRequest, MAX_NOTE_LEN, MAX_RATER_LEN, MAX_SCORE};
pub use tests_registry::{
    CaptureRules, MetricDef, ParameterDef, ParameterKind, TestDetail, TestList, TestStatus,
    TestSummary,
};
pub use video::{
    GateCheck, GateCheckName, GateStatus, Hand, UploadMeta, UploadResponse, Video, VideoList,
    MAX_PARTICIPANT_CODE_LEN,
};
