//! The `plugin.toml` file that describes a test plug-in, and its validation.
//!
//! A manifest is parsed strictly: an unknown key is an error, so a typo is caught instead of
//! being silently ignored. [`parse`] reports every problem it finds, not just the first.

use std::collections::HashSet;

use axonhand_api::{
    CaptureRules, MetricDef, ParameterDef, ParameterKind, TestDetail, TestStatus, TestSummary,
};
use serde::Deserialize;

/// A validated manifest.
#[derive(Debug, Clone, PartialEq)]
pub struct Manifest {
    /// What the API shows about the test.
    pub detail: TestDetail,
    /// Script to run, relative to the plug-in folder. `None` for a planned test.
    pub entrypoint: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawManifest {
    id: String,
    version: String,
    name: String,
    clinical_item: String,
    status: RawStatus,
    summary: String,
    entrypoint: Option<String>,
    capture: RawCapture,
    #[serde(default)]
    metrics: Vec<RawMetric>,
    #[serde(default)]
    parameters: Vec<RawParameter>,
}

#[derive(Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum RawStatus {
    Available,
    Planned,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCapture {
    min_fps: f64,
    min_duration_seconds: f64,
    #[serde(default)]
    board_required: bool,
    instructions: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMetric {
    id: String,
    name: String,
    #[serde(default)]
    unit: String,
    description: String,
    #[serde(default)]
    default_selected: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawParameter {
    id: String,
    name: String,
    description: String,
    kind: ParameterKind,
    default: f64,
    min: Option<f64>,
    max: Option<f64>,
}

/// True for `a`–`z` start, then lowercase letters, digits and `_`, at most 48 characters.
#[must_use]
pub fn is_valid_id(id: &str) -> bool {
    let mut chars = id.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && id.len() <= 48
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn is_version(text: &str) -> bool {
    let parts: Vec<&str> = text.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.len() <= 6 && p.bytes().all(|b| b.is_ascii_digit()))
}

/// A path that stays inside the plug-in folder: relative, no `..`, no drive or root.
fn is_safe_relative_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with(['/', '\\'])
        && !path.contains(':')
        && path
            .split(['/', '\\'])
            .all(|part| !part.is_empty() && part != "..")
}

/// Parse and validate manifest text.
///
/// # Errors
///
/// Every problem found, one sentence each, naming the key at fault.
pub fn parse(text: &str) -> Result<Manifest, Vec<String>> {
    let raw: RawManifest = toml::from_str(text).map_err(|e| vec![e.message().to_owned()])?;
    let mut problems = Vec::new();
    check_identity(&raw, &mut problems);
    check_capture(&raw.capture, &mut problems);
    check_metrics(&raw.metrics, &mut problems);
    check_parameters(&raw.parameters, &mut problems);
    match (raw.status, raw.entrypoint.as_deref()) {
        (RawStatus::Available, None) => {
            problems.push("entrypoint is required for an available test".to_owned());
        }
        (RawStatus::Available, Some(path)) if !is_safe_relative_path(path) => {
            problems
                .push("entrypoint must be a relative path inside the plug-in folder".to_owned());
        }
        (RawStatus::Planned, Some(_)) => {
            problems.push("a planned test must not have an entrypoint".to_owned());
        }
        _ => {}
    }
    if !problems.is_empty() {
        return Err(problems);
    }
    let status = match raw.status {
        RawStatus::Available => TestStatus::Available,
        RawStatus::Planned => TestStatus::Planned,
    };
    Ok(Manifest {
        entrypoint: raw.entrypoint,
        detail: TestDetail {
            summary: TestSummary {
                id: raw.id,
                version: raw.version,
                name: raw.name,
                clinical_item: raw.clinical_item,
                status,
                summary: raw.summary,
                error: None,
            },
            capture: CaptureRules {
                min_fps: raw.capture.min_fps,
                min_duration_seconds: raw.capture.min_duration_seconds,
                board_required: raw.capture.board_required,
                instructions: raw.capture.instructions,
            },
            metrics: raw
                .metrics
                .into_iter()
                .map(|m| MetricDef {
                    id: m.id,
                    name: m.name,
                    unit: m.unit,
                    description: m.description,
                    default_selected: m.default_selected,
                })
                .collect(),
            parameters: raw
                .parameters
                .into_iter()
                .map(|p| ParameterDef {
                    id: p.id,
                    name: p.name,
                    description: p.description,
                    kind: p.kind,
                    default: p.default,
                    min: p.min,
                    max: p.max,
                })
                .collect(),
        },
    })
}

fn check_identity(raw: &RawManifest, problems: &mut Vec<String>) {
    if !is_valid_id(&raw.id) {
        problems.push("id must start with a lowercase letter and use only lowercase letters, digits and '_' (48 characters at most)".to_owned());
    }
    if !is_version(&raw.version) {
        problems.push("version must look like 1.2.3".to_owned());
    }
    for (key, value) in [
        ("name", &raw.name),
        ("clinical_item", &raw.clinical_item),
        ("summary", &raw.summary),
    ] {
        if value.trim().is_empty() {
            problems.push(format!("{key} must not be empty"));
        }
    }
}

fn check_capture(capture: &RawCapture, problems: &mut Vec<String>) {
    if !(capture.min_fps.is_finite() && capture.min_fps > 0.0) {
        problems.push("capture.min_fps must be a positive number".to_owned());
    }
    if !(capture.min_duration_seconds.is_finite() && capture.min_duration_seconds > 0.0) {
        problems.push("capture.min_duration_seconds must be a positive number".to_owned());
    }
    if capture.instructions.trim().is_empty() {
        problems.push("capture.instructions must not be empty".to_owned());
    }
}

fn check_metrics(metrics: &[RawMetric], problems: &mut Vec<String>) {
    let mut seen = HashSet::new();
    for metric in metrics {
        if !is_valid_id(&metric.id) {
            problems.push(format!(
                "metric id {:?} is not a valid identifier",
                metric.id
            ));
        } else if !seen.insert(metric.id.as_str()) {
            problems.push(format!("metric id {:?} appears twice", metric.id));
        }
        if metric.name.trim().is_empty() || metric.description.trim().is_empty() {
            problems.push(format!(
                "metric {:?} needs a name and a description",
                metric.id
            ));
        }
    }
}

fn check_parameters(parameters: &[RawParameter], problems: &mut Vec<String>) {
    let mut seen = HashSet::new();
    for parameter in parameters {
        let id = &parameter.id;
        if !is_valid_id(id) {
            problems.push(format!("parameter id {id:?} is not a valid identifier"));
        } else if !seen.insert(id.as_str()) {
            problems.push(format!("parameter id {id:?} appears twice"));
        }
        if parameter.name.trim().is_empty() || parameter.description.trim().is_empty() {
            problems.push(format!("parameter {id:?} needs a name and a description"));
        }
        let finite = |v: Option<f64>| v.is_none_or(f64::is_finite);
        if !parameter.default.is_finite() || !finite(parameter.min) || !finite(parameter.max) {
            problems.push(format!(
                "parameter {id:?} has a value that is not a finite number"
            ));
            continue;
        }
        if let (Some(min), Some(max)) = (parameter.min, parameter.max) {
            if min > max {
                problems.push(format!("parameter {id:?} has min above max"));
            }
        }
        if parameter.min.is_some_and(|m| parameter.default < m)
            || parameter.max.is_some_and(|m| parameter.default > m)
        {
            problems.push(format!("parameter {id:?} has a default outside its range"));
        }
        if parameter.kind == ParameterKind::Integer && parameter.default.fract() != 0.0 {
            problems.push(format!(
                "parameter {id:?} is an integer but its default is not"
            ));
        }
        if parameter.kind == ParameterKind::Boolean
            && parameter.default != 0.0
            && parameter.default != 1.0
        {
            problems.push(format!(
                "parameter {id:?} is a boolean, so its default must be 0 or 1"
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) const GOOD: &str = r#"
id = "finger_tapping"
version = "0.1.0"
name = "Finger tapping"
clinical_item = "MDS-UPDRS 3.4"
status = "available"
summary = "Thumb and index finger tapping."
entrypoint = "plugin.py"

[capture]
min_fps = 50
min_duration_seconds = 10
board_required = false
instructions = "Film the hand from the front."

[[metrics]]
id = "tap_rate"
name = "Tap rate"
unit = "taps/s"
description = "Taps per second."
default_selected = true

[[parameters]]
id = "min_amplitude"
name = "Smallest tap"
description = "Taps smaller than this are ignored."
kind = "number"
default = 0.1
min = 0
max = 1
"#;

    fn problems(text: &str) -> Vec<String> {
        parse(text).err().unwrap_or_default()
    }

    fn with(replace_from: &str, replace_to: &str) -> String {
        GOOD.replacen(replace_from, replace_to, 1)
    }

    #[test]
    fn a_good_manifest_parses() -> Result<(), Vec<String>> {
        let manifest = parse(GOOD)?;
        assert_eq!(manifest.detail.summary.id, "finger_tapping");
        assert_eq!(manifest.detail.summary.status, TestStatus::Available);
        assert_eq!(manifest.entrypoint.as_deref(), Some("plugin.py"));
        assert_eq!(manifest.detail.metrics.len(), 1);
        assert_eq!(manifest.detail.parameters[0].max, Some(1.0));
        Ok(())
    }

    #[test]
    fn unknown_keys_are_rejected() {
        assert!(!problems(&format!("colour = \"red\"\n{GOOD}")).is_empty());
        assert!(!problems(&with("min_fps = 50", "min_fps = 50\nmax_fps = 90")).is_empty());
    }

    #[test]
    fn syntax_errors_and_missing_keys_are_reported() {
        assert!(!problems("this is not toml").is_empty());
        assert!(!problems(&with("name = \"Finger tapping\"\n", "")).is_empty());
    }

    #[test]
    fn bad_ids_and_versions_are_named() {
        for bad in ["Finger", "1abc", "has-dash", "", "a b"] {
            let text = with("id = \"finger_tapping\"", &format!("id = \"{bad}\""));
            assert!(
                problems(&text).iter().any(|p| p.starts_with("id ")),
                "{bad:?}"
            );
        }
        for bad in ["1.0", "v1.0.0", "1.0.0-beta", "1..0"] {
            let text = with("version = \"0.1.0\"", &format!("version = \"{bad}\""));
            assert!(
                problems(&text).iter().any(|p| p.starts_with("version")),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn entrypoints_must_stay_inside_the_plugin_folder() {
        for bad in [
            "../evil.py",
            "/etc/x.py",
            "C:\\x.py",
            "a/../b.py",
            "",
            "a//b.py",
        ] {
            let text = with(
                "entrypoint = \"plugin.py\"",
                &format!("entrypoint = '{bad}'"),
            );
            assert!(
                problems(&text).iter().any(|p| p.contains("entrypoint")),
                "{bad:?}"
            );
        }
        assert!(parse(&with(
            "entrypoint = \"plugin.py\"",
            "entrypoint = \"src/run.py\""
        ))
        .is_ok());
    }

    #[test]
    fn available_needs_an_entrypoint_and_planned_forbids_one() {
        let no_entry = with("entrypoint = \"plugin.py\"\n", "");
        assert!(problems(&no_entry).iter().any(|p| p.contains("required")));
        let planned_with_entry = with("status = \"available\"", "status = \"planned\"");
        assert!(problems(&planned_with_entry)
            .iter()
            .any(|p| p.contains("must not")));
        let planned = no_entry.replacen("status = \"available\"", "status = \"planned\"", 1);
        assert!(parse(&planned).is_ok());
    }

    #[test]
    fn duplicate_metric_and_parameter_ids_are_found() {
        let dup_metric =
            format!("{GOOD}\n[[metrics]]\nid = \"tap_rate\"\nname = \"x\"\ndescription = \"y\"\n");
        assert!(problems(&dup_metric)
            .iter()
            .any(|p| p.contains("appears twice")));
        let dup_param = format!("{GOOD}\n[[parameters]]\nid = \"min_amplitude\"\nname = \"x\"\ndescription = \"y\"\nkind = \"number\"\ndefault = 0.2\n");
        assert!(problems(&dup_param)
            .iter()
            .any(|p| p.contains("appears twice")));
    }

    #[test]
    fn parameter_ranges_and_types_are_checked() {
        let out_of_range = with("default = 0.1", "default = 5");
        assert!(problems(&out_of_range)
            .iter()
            .any(|p| p.contains("outside its range")));
        let inverted = with("min = 0\nmax = 1", "min = 2\nmax = 1");
        assert!(problems(&inverted)
            .iter()
            .any(|p| p.contains("min above max")));
        let fractional_int = with("kind = \"number\"", "kind = \"integer\"");
        assert!(problems(&fractional_int)
            .iter()
            .any(|p| p.contains("integer")));
        let bad_bool = with(
            "kind = \"number\"\ndefault = 0.1\nmin = 0\nmax = 1",
            "kind = \"boolean\"\ndefault = 0.5",
        );
        assert!(problems(&bad_bool).iter().any(|p| p.contains("boolean")));
        let nan = with("default = 0.1", "default = nan");
        assert!(problems(&nan).iter().any(|p| p.contains("finite")));
    }

    #[test]
    fn capture_rules_must_be_positive() {
        let bad = with("min_fps = 50", "min_fps = 0");
        assert!(problems(&bad).iter().any(|p| p.contains("min_fps")));
    }

    #[test]
    fn all_problems_are_reported_together() {
        let text = with("version = \"0.1.0\"", "version = \"x\"").replacen(
            "min_fps = 50",
            "min_fps = -1",
            1,
        );
        assert!(problems(&text).len() >= 2);
    }
}
