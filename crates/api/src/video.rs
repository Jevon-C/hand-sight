//! Video records, upload details and the quality gate report.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::ApiError;

/// Longest participant code accepted, in characters.
pub const MAX_PARTICIPANT_CODE_LEN: usize = 32;

/// Which hand was filmed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Hand {
    /// The participant's left hand.
    Left,
    /// The participant's right hand.
    Right,
}

/// Result of one quality gate check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum GateStatus {
    /// The video meets the requirement.
    Pass,
    /// The video does not meet the requirement.
    Fail,
    /// The check cannot be made yet; this is never reported as a pass.
    NotChecked,
}

/// What a quality gate check looks at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum GateCheckName {
    /// The frame rate is high enough to resolve finger taps.
    FrameRate,
    /// The clip is long enough to contain a full task.
    Duration,
    /// A hand is visible in the video.
    HandVisible,
    /// The calibration board is visible in the video.
    BoardVisible,
}

/// One line of the quality gate report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct GateCheck {
    /// What was checked.
    pub check: GateCheckName,
    /// Whether the video passed.
    pub status: GateStatus,
    /// A short plain-language explanation shown to the person who uploaded the video.
    pub detail: String,
}

/// Details a person must give when uploading a video.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct UploadMeta {
    /// Code identifying the participant. Letters, digits, `-` and `_` only; never a name.
    pub participant_code: String,
    /// Date of the visit, as `YYYY-MM-DD`.
    pub visit_date: String,
    /// Which hand was filmed.
    pub hand: Hand,
}

impl UploadMeta {
    /// Check the details and return them with surrounding spaces removed.
    ///
    /// # Errors
    ///
    /// Returns an `invalid_request` error naming the field that is wrong.
    pub fn validated(self) -> Result<Self, ApiError> {
        let code = self.participant_code.trim();
        if code.is_empty() {
            return Err(ApiError::invalid(
                "participant_code",
                "Enter a participant code.",
            ));
        }
        if code.chars().count() > MAX_PARTICIPANT_CODE_LEN {
            return Err(ApiError::invalid(
                "participant_code",
                format!("Use at most {MAX_PARTICIPANT_CODE_LEN} characters."),
            ));
        }
        if !code
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err(ApiError::invalid(
                "participant_code",
                "Use only letters, digits, '-' and '_'.",
            ));
        }
        let date = self.visit_date.trim();
        if !is_calendar_date(date) {
            return Err(ApiError::invalid(
                "visit_date",
                "Enter a real date as YYYY-MM-DD.",
            ));
        }
        Ok(Self {
            participant_code: code.to_owned(),
            visit_date: date.to_owned(),
            hand: self.hand,
        })
    }
}

/// True when `text` is a real calendar date written as `YYYY-MM-DD`.
fn is_calendar_date(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return false;
    }
    let digits = |range: std::ops::Range<usize>| -> Option<u32> {
        let part = &text[range];
        if part.bytes().all(|b| b.is_ascii_digit()) {
            part.parse().ok()
        } else {
            None
        }
    };
    let (Some(year), Some(month), Some(day)) = (digits(0..4), digits(5..7), digits(8..10)) else {
        return false;
    };
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    year >= 1900 && (1..=days).contains(&day)
}

/// A video in the staging area.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Video {
    /// Identifier: the SHA-256 of the file's bytes, in lowercase hex.
    pub id: String,
    /// Name the file had when uploaded.
    pub filename: String,
    /// Code identifying the participant.
    pub participant_code: String,
    /// Date of the visit, as `YYYY-MM-DD`.
    pub visit_date: String,
    /// Which hand was filmed.
    pub hand: Hand,
    /// When the video was uploaded, as UTC `YYYY-MM-DDTHH:MM:SSZ`.
    pub uploaded_at: String,
    /// Size of the file in bytes.
    pub size_bytes: u64,
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
    /// Frames per second.
    pub fps: f64,
    /// Length in seconds.
    pub duration_seconds: f64,
    /// Number of frames.
    pub frame_count: u32,
    /// Video codec name, for example `h264`.
    pub codec: String,
    /// Quality gate report, one entry per check.
    pub gate: Vec<GateCheck>,
}

/// Body of `GET /api/videos`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct VideoList {
    /// Videos matching the filters, newest first.
    pub videos: Vec<Video>,
}

/// Body of a successful `POST /api/videos`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct UploadResponse {
    /// The stored video.
    pub video: Video,
    /// True when an identical file was already in the staging area, so nothing new was stored.
    pub duplicate: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(code: &str, date: &str) -> UploadMeta {
        UploadMeta {
            participant_code: code.to_owned(),
            visit_date: date.to_owned(),
            hand: Hand::Left,
        }
    }

    #[test]
    fn good_details_are_trimmed_and_kept() {
        let ok = meta("  P-001_a ", " 2026-02-28 ").validated();
        assert_eq!(
            ok.map(|m| (m.participant_code, m.visit_date)).ok(),
            Some(("P-001_a".to_owned(), "2026-02-28".to_owned()))
        );
    }

    #[test]
    fn bad_participant_codes_name_the_field() {
        for code in [
            "",
            "   ",
            "has space",
            "ünï",
            "a/b",
            "../x",
            &"x".repeat(33),
        ] {
            let err = meta(code, "2026-01-01").validated().err();
            assert_eq!(
                err.and_then(|e| e.field),
                Some("participant_code".to_owned()),
                "{code:?}"
            );
        }
    }

    #[test]
    fn bad_dates_name_the_field() {
        for date in [
            "",
            "2026-13-01",
            "2026-02-29",
            "2026-1-01",
            "20260101",
            "2026-04-31",
            "abcd-ef-gh",
            "1800-01-01",
            "2026-00-10",
            "2026-01-00",
        ] {
            let err = meta("P1", date).validated().err();
            assert_eq!(
                err.and_then(|e| e.field),
                Some("visit_date".to_owned()),
                "{date:?}"
            );
        }
    }

    #[test]
    fn leap_days_are_handled() {
        assert!(meta("P1", "2028-02-29").validated().is_ok());
        assert!(meta("P1", "2000-02-29").validated().is_ok());
        assert!(meta("P1", "1900-02-29").validated().is_err());
    }

    #[test]
    fn enums_use_snake_case() -> Result<(), serde_json::Error> {
        assert_eq!(
            serde_json::to_string(&GateStatus::NotChecked)?,
            "\"not_checked\""
        );
        assert_eq!(
            serde_json::to_string(&GateCheckName::HandVisible)?,
            "\"hand_visible\""
        );
        assert_eq!(serde_json::to_string(&Hand::Right)?, "\"right\"");
        Ok(())
    }
}
