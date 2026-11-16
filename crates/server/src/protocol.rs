//! The files exchanged with a plug-in process, and the checks applied to them.
//!
//! The server writes `job.json`; the plug-in writes `result.json` (or exits with an error,
//! optionally explaining in `error.json`). Nothing a plug-in returns is trusted: every result
//! is checked against the manifest before it is used. See `docs/PLUGIN-PROTOCOL.md`.

use std::collections::{BTreeMap, HashSet};

use axonhand_api::{ParameterKind, QualityCheck, Signals, TestDetail};
use serde::{Deserialize, Serialize};

/// Version of the job and result file formats.
pub const SCHEMA_VERSION: u32 = 1;

/// What the server tells a plug-in to do.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct JobFile {
    /// Always [`SCHEMA_VERSION`].
    pub schema_version: u32,
    /// Identifier of this run.
    pub job_id: String,
    /// Identifier of the test.
    pub test_id: String,
    /// Absolute path of the video file to analyse.
    pub video_path: String,
    /// Absolute path of the landmark track (`.npz`) the hand model found in the video.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub landmarks_path: Option<String>,
    /// Metric ids to compute.
    pub metrics: Vec<String>,
    /// Every setting of the test, defaults filled in. Booleans are 0 or 1.
    pub parameters: BTreeMap<String, f64>,
}

/// One number reported by a plug-in.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Measure {
    /// Which metric this is.
    pub metric_id: String,
    /// The value, or `None` when it could not be measured.
    pub value: Option<f64>,
    /// Why there is no value. Required when `value` is `None`.
    #[serde(default)]
    pub reason: Option<String>,
}

/// What a plug-in returns.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginResult {
    /// Must equal [`SCHEMA_VERSION`].
    pub schema_version: u32,
    /// One entry per requested metric.
    pub measures: Vec<Measure>,
    /// Free-text remarks for the clinician, for example about filming quality.
    #[serde(default)]
    pub notes: Vec<String>,
    /// Checks on how far the measures can be trusted.
    #[serde(default)]
    pub quality: Vec<QualityCheck>,
}

/// The file of plot data a plug-in may write next to its result.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignalsFile {
    /// Must equal [`SCHEMA_VERSION`].
    pub schema_version: u32,
    /// Regularly sampled signals.
    pub series: Vec<axonhand_api::SeriesData>,
    /// Marked moments.
    pub events: Vec<axonhand_api::EventData>,
}

/// Check a signals file and turn it into the shared type.
///
/// # Errors
///
/// [`Refusal`] if the version is wrong, a step is not positive, a value is not finite, or an
/// event list has different numbers of times and values.
pub fn validate_signals(file: SignalsFile) -> Result<Signals, Refusal> {
    if file.schema_version != SCHEMA_VERSION {
        return Err(refuse("The signals file has an unknown format version."));
    }
    for series in &file.series {
        if !(series.step_seconds.is_finite() && series.step_seconds > 0.0)
            || !series.start_seconds.is_finite()
            || series.values.iter().any(|v| !v.is_finite())
        {
            return Err(refuse(format!(
                "The signal {:?} has invalid numbers.",
                series.id
            )));
        }
    }
    for events in &file.events {
        if events.times.len() != events.values.len()
            || events
                .times
                .iter()
                .chain(&events.values)
                .any(|v| !v.is_finite())
        {
            return Err(refuse(format!(
                "The events {:?} have invalid numbers.",
                events.id
            )));
        }
    }
    Ok(Signals {
        series: file.series,
        events: file.events,
    })
}

/// Why a result or a set of settings was refused. The text is safe to show to the user.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct Refusal(pub String);

fn refuse(message: impl Into<String>) -> Refusal {
    Refusal(message.into())
}

/// Check the settings a person chose and fill in defaults for the rest.
///
/// # Errors
///
/// [`Refusal`] for an unknown setting, a value out of range, a fraction for an integer
/// setting, or a boolean that is not 0 or 1.
pub fn complete_parameters(
    test: &TestDetail,
    given: &BTreeMap<String, f64>,
) -> Result<BTreeMap<String, f64>, Refusal> {
    let known: HashSet<&str> = test.parameters.iter().map(|p| p.id.as_str()).collect();
    if let Some(unknown) = given.keys().find(|k| !known.contains(k.as_str())) {
        return Err(refuse(format!("Unknown setting {unknown:?}.")));
    }
    let mut complete = BTreeMap::new();
    for parameter in &test.parameters {
        let value = given
            .get(&parameter.id)
            .copied()
            .unwrap_or(parameter.default);
        let bad = |why: &str| refuse(format!("{}: {why}", parameter.name));
        if !value.is_finite() {
            return Err(bad("must be a finite number."));
        }
        match parameter.kind {
            ParameterKind::Boolean if value != 0.0 && value != 1.0 => {
                return Err(bad("must be on or off."));
            }
            ParameterKind::Integer if value.fract() != 0.0 => {
                return Err(bad("must be a whole number."));
            }
            _ => {}
        }
        if parameter.min.is_some_and(|m| value < m) || parameter.max.is_some_and(|m| value > m) {
            return Err(bad("is outside the allowed range."));
        }
        complete.insert(parameter.id.clone(), value);
    }
    Ok(complete)
}

/// Check a requested metric list: known ids, none repeated, at least one.
///
/// # Errors
///
/// [`Refusal`] describing the first problem.
pub fn check_requested_metrics(test: &TestDetail, requested: &[String]) -> Result<(), Refusal> {
    if requested.is_empty() {
        return Err(refuse("Choose at least one metric."));
    }
    let known: HashSet<&str> = test.metrics.iter().map(|m| m.id.as_str()).collect();
    let mut seen = HashSet::new();
    for id in requested {
        if !known.contains(id.as_str()) {
            return Err(refuse(format!("This test has no metric {id:?}.")));
        }
        if !seen.insert(id.as_str()) {
            return Err(refuse(format!("Metric {id:?} was chosen twice.")));
        }
    }
    Ok(())
}

/// Check a plug-in's result against what was requested.
///
/// # Errors
///
/// [`Refusal`] if the schema version is wrong, a measure is for an unrequested or repeated
/// metric, a requested metric is missing, a value is not finite, or a missing value has no
/// reason.
pub fn validate_result(requested: &[String], result: &PluginResult) -> Result<(), Refusal> {
    if result.schema_version != SCHEMA_VERSION {
        return Err(refuse(format!(
            "The plug-in answered with format version {}, expected {SCHEMA_VERSION}.",
            result.schema_version
        )));
    }
    let wanted: HashSet<&str> = requested.iter().map(String::as_str).collect();
    let mut seen = HashSet::new();
    for measure in &result.measures {
        if !wanted.contains(measure.metric_id.as_str()) {
            return Err(refuse(format!(
                "The plug-in returned a metric that was not requested: {:?}.",
                measure.metric_id
            )));
        }
        if !seen.insert(measure.metric_id.as_str()) {
            return Err(refuse(format!(
                "The plug-in returned {:?} twice.",
                measure.metric_id
            )));
        }
        match (measure.value, measure.reason.as_deref()) {
            (Some(v), _) if !v.is_finite() => {
                return Err(refuse(format!(
                    "The value for {:?} is not a finite number.",
                    measure.metric_id
                )));
            }
            (None, None | Some("")) => {
                return Err(refuse(format!(
                    "{:?} has no value and no reason.",
                    measure.metric_id
                )));
            }
            _ => {}
        }
    }
    if let Some(missing) = requested.iter().find(|id| !seen.contains(id.as_str())) {
        return Err(refuse(format!(
            "The plug-in did not return metric {missing:?}."
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::parse;

    fn test_detail() -> Result<TestDetail, Vec<String>> {
        let text = r#"
id = "t"
version = "1.0.0"
name = "T"
clinical_item = "X"
status = "available"
summary = "s"
entrypoint = "p.py"
[capture]
min_fps = 50
min_duration_seconds = 5
instructions = "i"
[[metrics]]
id = "a"
name = "A"
description = "d"
[[metrics]]
id = "b"
name = "B"
description = "d"
[[parameters]]
id = "amp"
name = "Amplitude"
description = "d"
kind = "number"
default = 0.5
min = 0
max = 1
[[parameters]]
id = "n"
name = "Count"
description = "d"
kind = "integer"
default = 3
[[parameters]]
id = "flag"
name = "Flag"
description = "d"
kind = "boolean"
default = 0
"#;
        parse(text).map(|m| m.detail)
    }

    fn given(pairs: &[(&str, f64)]) -> BTreeMap<String, f64> {
        pairs.iter().map(|(k, v)| ((*k).to_owned(), *v)).collect()
    }

    #[test]
    fn defaults_fill_the_gaps() -> Result<(), Vec<String>> {
        let done =
            complete_parameters(&test_detail()?, &given(&[("amp", 0.9)])).map_err(|e| vec![e.0])?;
        assert_eq!(done, given(&[("amp", 0.9), ("n", 3.0), ("flag", 0.0)]));
        Ok(())
    }

    #[test]
    fn bad_settings_are_refused_with_a_reason() -> Result<(), Vec<String>> {
        let test = test_detail()?;
        for (pairs, expect) in [
            (vec![("nope", 1.0)], "Unknown setting"),
            (vec![("amp", 2.0)], "outside the allowed range"),
            (vec![("amp", -0.1)], "outside the allowed range"),
            (vec![("amp", f64::NAN)], "finite"),
            (vec![("n", 2.5)], "whole number"),
            (vec![("flag", 0.5)], "on or off"),
        ] {
            let error = complete_parameters(&test, &given(&pairs)).err();
            assert!(error.is_some_and(|e| e.0.contains(expect)), "{pairs:?}");
        }
        Ok(())
    }

    #[test]
    fn requested_metrics_are_checked() -> Result<(), Vec<String>> {
        let test = test_detail()?;
        let ids = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert!(check_requested_metrics(&test, &ids(&["a", "b"])).is_ok());
        assert!(check_requested_metrics(&test, &ids(&[])).is_err());
        assert!(check_requested_metrics(&test, &ids(&["zzz"])).is_err());
        assert!(check_requested_metrics(&test, &ids(&["a", "a"])).is_err());
        Ok(())
    }

    fn result(measures: Vec<Measure>) -> PluginResult {
        PluginResult {
            schema_version: 1,
            measures,
            notes: vec![],
            quality: vec![],
        }
    }

    fn value(id: &str, v: f64) -> Measure {
        Measure {
            metric_id: id.to_owned(),
            value: Some(v),
            reason: None,
        }
    }

    #[test]
    fn a_complete_result_is_accepted() {
        let wanted = vec!["a".to_owned(), "b".to_owned()];
        let missing_value = Measure {
            metric_id: "b".into(),
            value: None,
            reason: Some("no taps found".into()),
        };
        assert!(validate_result(&wanted, &result(vec![value("a", 1.0), missing_value])).is_ok());
    }

    #[test]
    fn bad_results_are_refused() {
        let wanted = vec!["a".to_owned(), "b".to_owned()];
        let cases = vec![
            (result(vec![value("a", 1.0)]), "did not return"),
            (
                result(vec![value("a", 1.0), value("b", 1.0), value("c", 1.0)]),
                "not requested",
            ),
            (
                result(vec![value("a", 1.0), value("a", 2.0), value("b", 1.0)]),
                "twice",
            ),
            (
                result(vec![value("a", f64::INFINITY), value("b", 1.0)]),
                "finite",
            ),
            (
                result(vec![
                    value("a", 1.0),
                    Measure {
                        metric_id: "b".into(),
                        value: None,
                        reason: None,
                    },
                ]),
                "no reason",
            ),
            (
                PluginResult {
                    schema_version: 2,
                    measures: vec![],
                    notes: vec![],
                    quality: vec![],
                },
                "version",
            ),
        ];
        for (candidate, expect) in cases {
            let error = validate_result(&wanted, &candidate).err();
            assert!(error.is_some_and(|e| e.0.contains(expect)), "{expect}");
        }
    }

    fn signals(text: &str) -> Result<Signals, Refusal> {
        let file: SignalsFile = serde_json::from_str(text).map_err(|e| Refusal(e.to_string()))?;
        validate_signals(file)
    }

    #[test]
    fn good_signals_pass_and_bad_ones_are_refused() {
        let good = r#"{"schema_version":1,"series":[{"id":"s","name":"S","unit":"u","start_seconds":0,"step_seconds":0.1,"values":[1,2,3]}],"events":[{"id":"e","name":"E","unit":"u","times":[0.1],"values":[2]}]}"#;
        assert!(signals(good).is_ok());
        for bad in [
            good.replace("\"step_seconds\":0.1", "\"step_seconds\":0"),
            good.replace("[1,2,3]", "[1,2,1e999]"),
            good.replace("\"times\":[0.1]", "\"times\":[0.1,0.2]"),
            good.replace("\"schema_version\":1", "\"schema_version\":9"),
        ] {
            assert!(signals(&bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn result_json_with_extra_keys_is_rejected() {
        let text = r#"{"schema_version":1,"measures":[],"surprise":1}"#;
        assert!(serde_json::from_str::<PluginResult>(text).is_err());
    }
}
