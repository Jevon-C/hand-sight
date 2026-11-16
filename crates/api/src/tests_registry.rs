//! Descriptions of the installed tests (plug-ins) and their metrics.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Whether a test can be run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum TestStatus {
    /// The test is installed and can be run.
    Available,
    /// The test is on the roadmap. It is listed so people can see it, and cannot be run.
    Planned,
    /// The plug-in is installed but its files are invalid; see the error.
    Broken,
}

/// Requirements a video must meet for a test to give meaningful numbers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct CaptureRules {
    /// Lowest frame rate, in frames per second.
    pub min_fps: f64,
    /// Shortest clip, in seconds.
    pub min_duration_seconds: f64,
    /// Whether the calibration board must be visible for results in centimetres.
    pub board_required: bool,
    /// Plain-language filming instructions shown to the person recording.
    pub instructions: String,
}

/// One number a test can report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct MetricDef {
    /// Stable identifier, for example `tap_rate`.
    pub id: String,
    /// Name shown to people.
    pub name: String,
    /// Unit of the value, for example `taps/s`. Empty for a pure count.
    pub unit: String,
    /// What the number means, in one or two sentences.
    pub description: String,
    /// Whether the metric is ticked by default when running the test.
    pub default_selected: bool,
}

/// The type of a test setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ParameterKind {
    /// Any real number.
    Number,
    /// A whole number.
    Integer,
    /// True or false.
    Boolean,
}

/// A setting a person may change before running a test.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ParameterDef {
    /// Stable identifier.
    pub id: String,
    /// Name shown to people.
    pub name: String,
    /// What the setting changes.
    pub description: String,
    /// The type of the value.
    pub kind: ParameterKind,
    /// Value used unless the person changes it. For `boolean`, 0 is false and 1 is true.
    pub default: f64,
    /// Smallest allowed value, for number and integer settings.
    pub min: Option<f64>,
    /// Largest allowed value, for number and integer settings.
    pub max: Option<f64>,
}

/// A line of the list at `GET /api/tests`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TestSummary {
    /// Stable identifier of the test.
    pub id: String,
    /// Version of the plug-in. Empty when the manifest could not be read.
    pub version: String,
    /// Name shown to people.
    pub name: String,
    /// The clinical item this test supports, for example `MDS-UPDRS 3.4`.
    pub clinical_item: String,
    /// Whether the test can be run.
    pub status: TestStatus,
    /// One-sentence description.
    pub summary: String,
    /// Why the plug-in is broken, when `status` is `broken`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Body of `GET /api/tests/{id}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TestDetail {
    /// The same facts as the list shows.
    pub summary: TestSummary,
    /// What a video must meet.
    pub capture: CaptureRules,
    /// The numbers this test can report.
    pub metrics: Vec<MetricDef>,
    /// Settings that may be changed.
    pub parameters: Vec<ParameterDef>,
}

/// Body of `GET /api/tests`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TestList {
    /// Every plug-in found, available, planned or broken, ordered by name.
    pub tests: Vec<TestSummary>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_uses_snake_case_and_error_is_optional() -> Result<(), serde_json::Error> {
        assert_eq!(serde_json::to_string(&TestStatus::Broken)?, "\"broken\"");
        let summary = TestSummary {
            id: "a".into(),
            version: "1.0.0".into(),
            name: "A".into(),
            clinical_item: "X".into(),
            status: TestStatus::Available,
            summary: "s".into(),
            error: None,
        };
        assert!(!serde_json::to_string(&summary)?.contains("error"));
        Ok(())
    }
}
