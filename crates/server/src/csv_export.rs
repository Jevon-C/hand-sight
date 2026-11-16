//! CSV export of a result.
//!
//! The file starts with comment lines (beginning with `#`) that state what the numbers are and
//! where they came from, so a spreadsheet opened months later still says which test version,
//! model and settings produced it and that it is a research measure, not a clinical score.

use axonhand_api::AnalysisResult;

/// A text cell made safe for spreadsheets: quoted when needed, and a leading `=`, `+`, `-` or
/// `@` (which a spreadsheet would run as a formula) is defused with a leading apostrophe.
fn text_cell(text: &str) -> String {
    let defused = if text.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{text}")
    } else {
        text.to_owned()
    };
    if defused.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", defused.replace('"', "\"\""))
    } else {
        defused
    }
}

fn comment(text: &str) -> String {
    // A comment is one line: line breaks in the text would start a line that is not a comment.
    format!("# {}\n", text.replace(['\r', '\n'], " "))
}

/// The CSV text for `result`.
#[must_use]
pub fn result_csv(result: &AnalysisResult) -> String {
    let mut out = String::new();
    out.push_str(&comment(
        "Hand Sight result. Research prototype: these are research measures, not clinical scores.",
    ));
    out.push_str(&comment(&format!(
        "test: {} (version {})",
        result.test_name, result.test_version
    )));
    out.push_str(&comment(&format!(
        "hand model: {}{}{}",
        result.model.name,
        if result.model.device.is_empty() {
            String::new()
        } else {
            format!(" on the {}", result.model.device)
        },
        if result.model.stand_in {
            " (stand-in for development)"
        } else {
            ""
        }
    )));
    if result.illustrative {
        out.push_str(&comment(
            "ILLUSTRATIVE: not from a validated pipeline; do not interpret.",
        ));
    }
    out.push_str(&comment(&format!("video: {}", result.video_id)));
    out.push_str(&comment(&format!("result key: {}", result.key)));
    out.push_str(&comment(&format!("created: {}", result.created_at)));
    let settings: Vec<String> = result
        .parameters
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect();
    out.push_str(&comment(&format!("settings: {}", settings.join("; "))));
    for check in &result.quality {
        out.push_str(&comment(&format!(
            "quality {:?}: {}",
            check.status, check.detail
        )));
    }
    out.push_str("metric,name,value,unit,reason\n");
    for measure in &result.measures {
        let value = measure.value.map_or(String::new(), |v| format!("{v}"));
        out.push_str(&format!(
            "{},{},{},{},{}\n",
            text_cell(&measure.metric_id),
            text_cell(&measure.name),
            value,
            text_cell(&measure.unit),
            text_cell(measure.reason.as_deref().unwrap_or("")),
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use axonhand_api::{MeasureValue, ModelInfo, QualityCheck, QualityStatus};
    use std::collections::BTreeMap;

    fn result() -> AnalysisResult {
        AnalysisResult {
            key: "k".into(),
            video_id: "v".into(),
            test_id: "t".into(),
            test_name: "Finger tapping".into(),
            test_version: "0.1.0".into(),
            model: ModelInfo {
                name: "Stand-in".into(),
                hash: "h".into(),
                stand_in: true,
                device: String::new(),
            },
            parameters: BTreeMap::from([("min_amplitude".to_owned(), 0.15)]),
            measures: vec![
                MeasureValue {
                    metric_id: "tap_rate".into(),
                    name: "Tap rate".into(),
                    unit: "taps/s".into(),
                    value: Some(3.25),
                    reason: None,
                },
                MeasureValue {
                    metric_id: "arrests".into(),
                    name: "Arrests".into(),
                    unit: String::new(),
                    value: None,
                    reason: Some("no taps, sorry".into()),
                },
            ],
            quality: vec![QualityCheck {
                name: "q".into(),
                status: QualityStatus::Flag,
                detail: "few taps".into(),
            }],
            notes: vec![],
            illustrative: true,
            created_at: "2026-01-01T00:00:00Z".into(),
        }
    }

    #[test]
    fn the_header_states_what_the_numbers_are() {
        let csv = result_csv(&result());
        for expected in [
            "research measures, not clinical scores",
            "version 0.1.0",
            "stand-in",
            "ILLUSTRATIVE",
            "min_amplitude=0.15",
            "result key: k",
        ] {
            assert!(csv.contains(expected), "{expected}");
        }
    }

    #[test]
    fn rows_follow_the_header_with_values_and_reasons() {
        let csv = result_csv(&result());
        let rows: Vec<&str> = csv.lines().filter(|l| !l.starts_with('#')).collect();
        assert_eq!(rows[0], "metric,name,value,unit,reason");
        assert_eq!(rows[1], "tap_rate,Tap rate,3.25,taps/s,");
        assert_eq!(rows[2], "arrests,Arrests,,,\"no taps, sorry\"");
    }

    #[test]
    fn formulas_are_defused_and_quotes_escaped() {
        assert_eq!(text_cell("=SUM(A1)"), "'=SUM(A1)");
        assert_eq!(text_cell("+1"), "'+1");
        assert_eq!(text_cell("@x"), "'@x");
        assert_eq!(text_cell("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(text_cell("plain"), "plain");
    }

    #[test]
    fn a_line_break_cannot_escape_a_comment() {
        let mut r = result();
        r.test_name = "Name\nmetric,injected".into();
        let csv = result_csv(&r);
        assert!(csv
            .lines()
            .all(|l| l.starts_with('#') || !l.contains("injected")));
    }

    #[test]
    fn a_result_that_is_not_illustrative_has_no_such_line() {
        let mut r = result();
        r.illustrative = false;
        r.model.stand_in = false;
        assert!(!result_csv(&r).contains("ILLUSTRATIVE"));
    }
}
