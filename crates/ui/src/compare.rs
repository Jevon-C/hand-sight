//! Putting two results of the same test side by side. No browser dependency, so the rules can
//! be unit-tested natively.

use axonhand_api::{AnalysisResult, ClinicianScore, MeasureValue, ResultSummary};

use crate::format::{hand_label, measure_text, score_label};

/// One measure in both results.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    /// Name shown to people.
    pub name: String,
    /// The first result's value, ready to show.
    pub first: String,
    /// The second result's value, ready to show.
    pub second: String,
    /// How the second differs from the first, ready to show; a dash when it cannot be said.
    pub change: String,
}

/// Two results compared.
#[derive(Debug, Clone, PartialEq)]
pub struct Comparison {
    /// One row for each measure of the first result.
    pub rows: Vec<Row>,
    /// Reasons to read the differences with care, in plain words.
    pub cautions: Vec<String>,
}

/// Signed number for a difference: up to three decimals, always with its sign, with the unit.
fn signed(value: f64, unit: &str) -> String {
    let text = format!("{value:+.3}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    let text = if text == "+0" || text == "-0" {
        "0"
    } else {
        text
    };
    if unit.is_empty() {
        text.to_owned()
    } else {
        format!("{text} {unit}")
    }
}

/// How `second` differs from `first`: the difference with its unit and, when the first value is
/// not zero, the relative change in percent.
#[must_use]
pub fn change_text(first: Option<f64>, second: Option<f64>, unit: &str) -> String {
    let (Some(a), Some(b)) = (
        first.filter(|v| v.is_finite()),
        second.filter(|v| v.is_finite()),
    ) else {
        return "—".to_owned();
    };
    let difference = signed(b - a, unit);
    if a == 0.0 {
        difference
    } else {
        format!("{difference} ({:+.0}%)", (b - a) / a.abs() * 100.0)
    }
}

/// One rater's scores of the two results.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScoreRow {
    /// Who gave the scores.
    pub rater: String,
    /// The score of the first result, ready to show; a dash when this rater gave none.
    pub first: String,
    /// The score of the second result, ready to show; a dash when this rater gave none.
    pub second: String,
    /// The second score minus the first, with its sign; a dash unless the rater scored both.
    pub change: String,
}

/// Line up the scores of two results by rater, ordered by name. A rater who scored only one of
/// them still gets a row, so a missing score is visible and not mistaken for a zero.
#[must_use]
pub fn score_rows(first: &[ClinicianScore], second: &[ClinicianScore]) -> Vec<ScoreRow> {
    let mut raters: Vec<&str> = first
        .iter()
        .chain(second)
        .map(|s| s.rater.as_str())
        .collect();
    raters.sort_unstable();
    raters.dedup();
    raters
        .into_iter()
        .map(|rater| {
            let find =
                |list: &[ClinicianScore]| list.iter().find(|s| s.rater == rater).map(|s| s.score);
            let (a, b) = (find(first), find(second));
            let show = |score: Option<u8>| score.map_or_else(|| "—".to_owned(), score_label);
            let change = match (a, b) {
                (Some(a), Some(b)) => match i16::from(b) - i16::from(a) {
                    0 => "0".to_owned(),
                    d => format!("{d:+}"),
                },
                _ => "—".to_owned(),
            };
            ScoreRow {
                rater: rater.to_owned(),
                first: show(a),
                second: show(b),
                change,
            }
        })
        .collect()
}

fn value_of<'a>(result: &'a AnalysisResult, metric_id: &str) -> Option<&'a MeasureValue> {
    result.measures.iter().find(|m| m.metric_id == metric_id)
}

fn differing_settings(first: &AnalysisResult, second: &AnalysisResult) -> Vec<String> {
    let mut names: Vec<&String> = first
        .parameters
        .keys()
        .chain(second.parameters.keys())
        .collect();
    names.sort();
    names.dedup();
    names
        .into_iter()
        .filter(|name| first.parameters.get(*name) != second.parameters.get(*name))
        .cloned()
        .collect()
}

/// Compare two results.
///
/// # Errors
///
/// A sentence fit to show the user when the results come from different tests, because their
/// numbers do not mean the same thing.
pub fn compare(first: &AnalysisResult, second: &AnalysisResult) -> Result<Comparison, String> {
    if first.test_id != second.test_id {
        return Err(format!(
            "These results come from different tests ({} and {}), so their numbers cannot be compared.",
            first.test_name, second.test_name
        ));
    }
    let mut cautions = Vec::new();
    if first.key == second.key {
        cautions.push("Both sides are the same result.".to_owned());
    } else if first.video_id == second.video_id {
        cautions.push("Both results are from the same video.".to_owned());
    }
    if first.test_version != second.test_version {
        cautions.push(format!(
            "The test changed between these results (version {} and {}). Its definitions may differ.",
            first.test_version, second.test_version
        ));
    }
    if first.model.hash != second.model.hash
        && first.model.name == second.model.name
        && first.model.device != second.model.device
    {
        cautions.push(format!(
            "The hand model ran on the {} for one result and the {} for the other. The NPU \
             computes with less precision, so small differences in the hand positions, and in \
             the numbers measured from them, are expected.",
            first.model.device, second.model.device
        ));
    } else if first.model.hash != second.model.hash {
        cautions.push(
            "The hand model differs between these results, so some of the difference may come from the model."
                .to_owned(),
        );
    }
    let settings = differing_settings(first, second);
    if !settings.is_empty() {
        cautions.push(format!("The settings differ: {}.", settings.join(", ")));
    }
    let rows = first
        .measures
        .iter()
        .map(|m| {
            let other = value_of(second, &m.metric_id);
            let other_value = other.and_then(|o| o.value);
            Row {
                name: m.name.clone(),
                first: measure_text(m.value, &m.unit),
                second: match other {
                    Some(o) => measure_text(o.value, &o.unit),
                    None => "Not in this result".to_owned(),
                },
                change: change_text(m.value, other_value, &m.unit),
            }
        })
        .collect();
    Ok(Comparison { rows, cautions })
}

/// One line describing a saved result in a list: who, when, which hand, which test.
#[must_use]
pub fn result_label(summary: &ResultSummary, test_name: &str) -> String {
    format!(
        "{} · {} · {} · {} {}",
        summary.participant_code,
        summary.visit_date,
        hand_label(summary.hand),
        test_name,
        summary.test_version
    )
}

/// The results that can be set against `first`: the same test, and not `first` itself.
#[must_use]
pub fn partners<'a>(all: &'a [ResultSummary], first: &str) -> Vec<&'a ResultSummary> {
    let Some(test) = all.iter().find(|r| r.key == first).map(|r| &r.test_id) else {
        return Vec::new();
    };
    all.iter()
        .filter(|r| &r.test_id == test && r.key != first)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axonhand_api::{Hand, ModelInfo};
    use std::collections::BTreeMap;

    fn measure(id: &str, value: Option<f64>) -> MeasureValue {
        MeasureValue {
            metric_id: id.to_owned(),
            name: id.to_uppercase(),
            unit: "Hz".to_owned(),
            value,
            reason: None,
        }
    }

    fn result(key: &str, video: &str, measures: Vec<MeasureValue>) -> AnalysisResult {
        AnalysisResult {
            key: key.to_owned(),
            video_id: video.to_owned(),
            test_id: "finger_tapping".to_owned(),
            test_name: "Finger tapping".to_owned(),
            test_version: "0.2.0".to_owned(),
            model: ModelInfo {
                name: "m".to_owned(),
                hash: "h".to_owned(),
                stand_in: false,
                device: String::new(),
            },
            parameters: BTreeMap::new(),
            measures,
            quality: Vec::new(),
            notes: Vec::new(),
            illustrative: false,
            created_at: "2026-03-01T10:00:00Z".to_owned(),
        }
    }

    fn summary(key: &str, test: &str) -> ResultSummary {
        ResultSummary {
            key: key.to_owned(),
            video_id: format!("v-{key}"),
            test_id: test.to_owned(),
            test_version: "0.2.0".to_owned(),
            created_at: "2026-03-01T10:00:00Z".to_owned(),
            participant_code: "P01".to_owned(),
            visit_date: "2026-03-01".to_owned(),
            hand: Hand::Left,
        }
    }

    #[test]
    fn a_change_shows_its_sign_unit_and_relative_size() {
        assert_eq!(change_text(Some(2.0), Some(2.5), "Hz"), "+0.5 Hz (+25%)");
        assert_eq!(change_text(Some(2.0), Some(1.5), "Hz"), "-0.5 Hz (-25%)");
        assert_eq!(change_text(Some(2.0), Some(2.0), "Hz"), "0 Hz (+0%)");
        assert_eq!(change_text(Some(-2.0), Some(-1.0), ""), "+1 (+50%)");
    }

    #[test]
    fn a_change_from_zero_has_no_percentage_and_a_missing_value_has_no_change() {
        assert_eq!(change_text(Some(0.0), Some(1.0), "Hz"), "+1 Hz");
        assert_eq!(change_text(None, Some(1.0), "Hz"), "—");
        assert_eq!(change_text(Some(1.0), None, "Hz"), "—");
        assert_eq!(change_text(Some(f64::NAN), Some(1.0), "Hz"), "—");
    }

    #[test]
    fn different_tests_cannot_be_compared() {
        let a = result("a", "v1", vec![]);
        let mut b = result("b", "v2", vec![]);
        b.test_id = "hand_open_close".to_owned();
        b.test_name = "Hand open and close".to_owned();
        let message = compare(&a, &b).err().unwrap_or_default();
        assert!(message.contains("different tests"), "{message}");
        assert!(message.contains("Finger tapping") && message.contains("Hand open and close"));
    }

    #[test]
    fn rows_follow_the_first_result_and_note_a_measure_the_second_lacks() -> Result<(), String> {
        let a = result(
            "a",
            "v1",
            vec![measure("rate", Some(2.0)), measure("extra", Some(1.0))],
        );
        let b = result("b", "v2", vec![measure("rate", Some(2.5))]);
        let compared = compare(&a, &b)?;
        assert_eq!(compared.rows.len(), 2);
        assert_eq!(compared.rows[0].first, "2 Hz");
        assert_eq!(compared.rows[0].second, "2.5 Hz");
        assert_eq!(compared.rows[0].change, "+0.5 Hz (+25%)");
        assert_eq!(compared.rows[1].second, "Not in this result");
        assert_eq!(compared.rows[1].change, "—");
        assert!(compared.cautions.is_empty());
        Ok(())
    }

    #[test]
    fn the_same_model_on_the_cpu_and_the_npu_is_explained_as_the_npus_precision(
    ) -> Result<(), String> {
        let mut a = result("a", "v1", vec![]);
        let mut b = result("b", "v1", vec![]);
        (a.model.device, b.model.device) = ("CPU".to_owned(), "NPU".to_owned());
        b.model.hash = "npu-hash".to_owned();
        let compared = compare(&a, &b)?;
        let all = compared.cautions.join(" ");
        assert!(all.contains("CPU for one result and the NPU"), "{all}");
        assert!(!all.contains("hand model differs"), "{all}");
        Ok(())
    }

    #[test]
    fn anything_that_could_explain_a_difference_is_called_out() -> Result<(), String> {
        let a = result("a", "v1", vec![]);
        let mut b = result("b", "v1", vec![]);
        b.test_version = "0.3.0".to_owned();
        b.model.hash = "other".to_owned();
        b.parameters.insert("band".to_owned(), 3.0);
        let compared = compare(&a, &b)?;
        let all = compared.cautions.join(" ");
        assert!(all.contains("same video"), "{all}");
        assert!(all.contains("0.2.0 and 0.3.0"), "{all}");
        assert!(all.contains("hand model differs"), "{all}");
        assert!(all.contains("settings differ: band"), "{all}");
        assert_eq!(compared.cautions.len(), 4);
        Ok(())
    }

    #[test]
    fn comparing_a_result_with_itself_is_said_plainly() -> Result<(), String> {
        let a = result("a", "v1", vec![]);
        let compared = compare(&a, &a)?;
        assert_eq!(compared.cautions, ["Both sides are the same result."]);
        Ok(())
    }

    fn given(rater: &str, score: u8) -> ClinicianScore {
        ClinicianScore {
            result_key: "k".to_owned(),
            rater: rater.to_owned(),
            score,
            note: String::new(),
            recorded_at: "2026-03-01T10:00:00Z".to_owned(),
        }
    }

    #[test]
    fn scores_are_lined_up_by_rater_and_a_missing_one_is_not_a_zero() {
        let rows = score_rows(
            &[given("CD", 2), given("AB", 3)],
            &[given("AB", 1), given("EF", 0)],
        );
        let shown: Vec<(&str, &str, &str, &str)> = rows
            .iter()
            .map(|r| {
                (
                    r.rater.as_str(),
                    r.first.as_str(),
                    r.second.as_str(),
                    r.change.as_str(),
                )
            })
            .collect();
        assert_eq!(
            shown,
            [
                ("AB", "3 · Moderate", "1 · Slight", "-2"),
                ("CD", "2 · Mild", "—", "—"),
                ("EF", "—", "0 · Normal", "—"),
            ]
        );
    }

    #[test]
    fn an_unchanged_score_shows_zero_and_no_scores_give_no_rows() {
        let rows = score_rows(&[given("AB", 2)], &[given("AB", 2)]);
        assert_eq!(rows[0].change, "0");
        assert!(score_rows(&[], &[]).is_empty());
    }

    #[test]
    fn partners_are_the_same_test_without_the_first_result() {
        let all = [
            summary("a", "finger_tapping"),
            summary("b", "finger_tapping"),
            summary("c", "hand_open_close"),
        ];
        let keys: Vec<&str> = partners(&all, "a").iter().map(|r| r.key.as_str()).collect();
        assert_eq!(keys, ["b"]);
        assert!(partners(&all, "missing").is_empty());
    }

    #[test]
    fn a_result_is_described_by_who_when_hand_and_test() {
        assert_eq!(
            result_label(&summary("a", "finger_tapping"), "Finger tapping"),
            "P01 · 2026-03-01 · Left hand · Finger tapping 0.2.0"
        );
    }
}
