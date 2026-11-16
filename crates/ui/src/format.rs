//! Plain-language formatting and request helpers with no browser dependency, so they can be
//! unit-tested natively.

use axonhand_api::{ApiError, Hand};

/// File size for people: `842 KB`, `12.4 MB`, `1.2 GB` (decimal units, as file managers show).
#[must_use]
pub fn file_size(bytes: u64) -> String {
    const KB: f64 = 1_000.0;
    let b = bytes as f64;
    if b < KB {
        format!("{bytes} B")
    } else if b < KB * KB {
        format!("{:.0} KB", b / KB)
    } else if b < KB * KB * KB {
        format!("{:.1} MB", b / (KB * KB))
    } else {
        format!("{:.1} GB", b / (KB * KB * KB))
    }
}

/// Length as `m:ss`, for example `1:05`.
#[must_use]
pub fn clip_length(seconds: f64) -> String {
    let whole = seconds.max(0.0).round() as u64;
    format!("{}:{:02}", whole / 60, whole % 60)
}

/// Hand as it is shown to people.
#[must_use]
pub fn hand_label(hand: Hand) -> &'static str {
    match hand {
        Hand::Left => "Left hand",
        Hand::Right => "Right hand",
    }
}

/// The value the server's `hand` field and query parameter use.
#[must_use]
pub fn hand_value(hand: Hand) -> &'static str {
    match hand {
        Hand::Left => "left",
        Hand::Right => "right",
    }
}

/// Query string for `GET /api/videos`, with values encoded. Empty filters are left out.
#[must_use]
pub fn list_query(participant_code: &str, hand: Option<Hand>) -> String {
    let mut parts = Vec::new();
    let code = participant_code.trim();
    if !code.is_empty() {
        parts.push(format!("participant_code={}", percent_encode(code)));
    }
    if let Some(hand) = hand {
        parts.push(format!("hand={}", hand_value(hand)));
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!("?{}", parts.join("&"))
    }
}

fn percent_encode(text: &str) -> String {
    use std::fmt::Write;
    text.bytes().fold(String::new(), |mut out, b| {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(b));
        } else {
            // Writing to a String cannot fail.
            let _ = write!(out, "%{b:02X}");
        }
        out
    })
}

/// A fraction between 0 and 1 as a whole percentage, kept within 0 to 100.
#[must_use]
pub fn progress_percent(fraction: f64) -> u32 {
    if fraction.is_finite() {
        (fraction.clamp(0.0, 1.0) * 100.0).round() as u32
    } else {
        0
    }
}

/// A measure's number alone: up to three decimals without trailing zeros. `None` and non-finite
/// values read `Not measured`.
#[must_use]
pub fn measure_number(value: Option<f64>) -> String {
    let Some(value) = value.filter(|v| v.is_finite()) else {
        return "Not measured".to_owned();
    };
    let text = format!("{value:.3}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    let text = if text == "-0" { "0" } else { text };
    text.to_owned()
}

/// A measure for people: its number, then its unit.
#[must_use]
pub fn measure_text(value: Option<f64>, unit: &str) -> String {
    let number = measure_number(value);
    if unit.is_empty() || value.is_none_or(|v| !v.is_finite()) {
        number
    } else {
        format!("{number} {unit}")
    }
}

/// An MDS-UPDRS part III score with its meaning, for example `3 · Moderate`.
#[must_use]
pub fn score_label(score: u8) -> String {
    let meaning = match score {
        0 => "Normal",
        1 => "Slight",
        2 => "Mild",
        3 => "Moderate",
        _ => "Severe",
    };
    format!("{score} · {meaning}")
}

/// A sentence for a failed request: the server's own message when the body is the shared
/// error shape, otherwise a generic one based on the status.
#[must_use]
pub fn failure_message(status: u16, body: &str) -> String {
    if let Ok(error) = serde_json::from_str::<ApiError>(body) {
        return error.message;
    }
    match status {
        0 => "Could not reach the server. Check that it is still running.".to_owned(),
        413 => "The video is too large.".to_owned(),
        _ => format!("The server could not complete the request (status {status})."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_use_decimal_units() {
        assert_eq!(file_size(999), "999 B");
        assert_eq!(file_size(842_000), "842 KB");
        assert_eq!(file_size(12_400_000), "12.4 MB");
        assert_eq!(file_size(1_200_000_000), "1.2 GB");
    }

    #[test]
    fn scores_carry_their_meaning() {
        assert_eq!(score_label(0), "0 · Normal");
        assert_eq!(score_label(3), "3 · Moderate");
        assert_eq!(score_label(4), "4 · Severe");
    }

    #[test]
    fn lengths_are_minutes_and_seconds() {
        assert_eq!(clip_length(65.0), "1:05");
        assert_eq!(clip_length(9.6), "0:10");
        assert_eq!(clip_length(-3.0), "0:00");
    }

    #[test]
    fn query_strings_skip_empty_filters_and_encode_values() {
        assert_eq!(list_query("", None), "");
        assert_eq!(list_query("  ", None), "");
        assert_eq!(
            list_query("P-1", Some(Hand::Left)),
            "?participant_code=P-1&hand=left"
        );
        assert_eq!(list_query("a b&c", None), "?participant_code=a%20b%26c");
        assert_eq!(list_query("", Some(Hand::Right)), "?hand=right");
    }

    #[test]
    fn progress_is_a_whole_percentage_within_bounds() {
        assert_eq!(progress_percent(0.0), 0);
        assert_eq!(progress_percent(0.456), 46);
        assert_eq!(progress_percent(1.7), 100);
        assert_eq!(progress_percent(-1.0), 0);
        assert_eq!(progress_percent(f64::NAN), 0);
    }

    #[test]
    fn measures_are_short_and_say_when_missing() {
        assert_eq!(measure_text(Some(3.25), "taps/s"), "3.25 taps/s");
        assert_eq!(measure_text(Some(12.0), ""), "12");
        assert_eq!(measure_text(Some(-0.0001), "%"), "0 %");
        assert_eq!(measure_text(Some(0.123_456), "x"), "0.123 x");
        assert_eq!(measure_text(None, "x"), "Not measured");
        assert_eq!(measure_number(Some(-9.2)), "-9.2");
        assert_eq!(measure_number(Some(0.588)), "0.588");
        assert_eq!(measure_number(None), "Not measured");
        assert_eq!(measure_text(Some(f64::NAN), "x"), "Not measured");
    }

    #[test]
    fn failures_prefer_the_servers_message() {
        let body =
            r#"{"code":"too_large","message":"The video is larger than the 2000 MB limit."}"#;
        assert_eq!(
            failure_message(413, body),
            "The video is larger than the 2000 MB limit."
        );
        assert!(failure_message(0, "").contains("Could not reach"));
        assert_eq!(failure_message(413, "<html>"), "The video is too large.");
        assert!(failure_message(502, "").contains("502"));
    }
}
