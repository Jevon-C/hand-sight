//! Scores a clinician gives to a result, for checking the measures against clinical practice.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::ApiError;

/// Highest score on the MDS-UPDRS part III items (0 is normal).
pub const MAX_SCORE: u8 = 4;
/// Longest rater name accepted, in characters.
pub const MAX_RATER_LEN: usize = 40;
/// Longest note accepted, in characters.
pub const MAX_NOTE_LEN: usize = 500;

/// Body of `PUT /api/results/{key}/scores`: one clinician's score for a result. Sending again
/// with the same rater replaces that rater's earlier score.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ScoreRequest {
    /// Who gave the score, for example initials. Several raters can score the same result.
    pub rater: String,
    /// The score, from 0 (normal) to 4 (severe).
    pub score: u8,
    /// An optional remark.
    #[serde(default)]
    pub note: String,
}

impl ScoreRequest {
    /// Trim the text and check every field.
    ///
    /// # Errors
    ///
    /// An `invalid_request` error naming the field that is wrong.
    pub fn validated(self) -> Result<Self, ApiError> {
        let rater = self.rater.trim();
        if rater.is_empty() {
            return Err(ApiError::invalid("rater", "Enter who is giving the score."));
        }
        if rater.chars().count() > MAX_RATER_LEN {
            return Err(ApiError::invalid(
                "rater",
                format!("Use at most {MAX_RATER_LEN} characters."),
            ));
        }
        if self.score > MAX_SCORE {
            return Err(ApiError::invalid(
                "score",
                format!("The score is from 0 to {MAX_SCORE}."),
            ));
        }
        let note = self.note.trim();
        if note.chars().count() > MAX_NOTE_LEN {
            return Err(ApiError::invalid(
                "note",
                format!("Use at most {MAX_NOTE_LEN} characters."),
            ));
        }
        Ok(Self {
            rater: rater.to_owned(),
            score: self.score,
            note: note.to_owned(),
        })
    }
}

/// A score as stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ClinicianScore {
    /// The result that was scored.
    pub result_key: String,
    /// Who gave the score.
    pub rater: String,
    /// The score, from 0 to 4.
    pub score: u8,
    /// An optional remark.
    pub note: String,
    /// When the score was last saved, as UTC `YYYY-MM-DDTHH:MM:SSZ`.
    pub recorded_at: String,
}

/// Body of `GET /api/results/{key}/scores`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ScoreList {
    /// The scores of this result, by rater name.
    pub scores: Vec<ClinicianScore>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(rater: &str, score: u8, note: &str) -> ScoreRequest {
        ScoreRequest {
            rater: rater.to_owned(),
            score,
            note: note.to_owned(),
        }
    }

    fn field(request: ScoreRequest) -> Option<String> {
        request.validated().err().and_then(|e| e.field)
    }

    #[test]
    fn good_scores_are_trimmed_and_kept() {
        let ok = request("  AB ", 3, " slow ").validated().ok();
        assert_eq!(ok, Some(request("AB", 3, "slow")));
        assert!(request("AB", 0, "").validated().is_ok());
        assert!(request("AB", MAX_SCORE, "").validated().is_ok());
    }

    #[test]
    fn a_bad_field_is_named() {
        assert_eq!(field(request("  ", 1, "")), Some("rater".to_owned()));
        assert_eq!(
            field(request(&"x".repeat(MAX_RATER_LEN + 1), 1, "")),
            Some("rater".to_owned())
        );
        assert_eq!(field(request("AB", 5, "")), Some("score".to_owned()));
        assert_eq!(
            field(request("AB", 1, &"x".repeat(MAX_NOTE_LEN + 1))),
            Some("note".to_owned())
        );
    }
}
