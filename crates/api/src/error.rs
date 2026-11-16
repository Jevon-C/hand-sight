//! The one error shape every endpoint uses.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Stable machine-readable reason for a failed request.
///
/// New codes may be added; clients must treat an unknown code as [`ErrorCode::Internal`].
/// Existing codes never change meaning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// The request was malformed or a field had an invalid value.
    InvalidRequest,
    /// The thing asked for does not exist.
    NotFound,
    /// The request conflicts with the current state, for example a duplicate.
    Conflict,
    /// The uploaded file is larger than the allowed size.
    TooLarge,
    /// The uploaded file is not of an accepted type.
    UnsupportedMedia,
    /// Something went wrong on the server. Details are in the server log, not the response.
    Internal,
}

/// Body of every error response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ApiError {
    /// Why the request failed.
    pub code: ErrorCode,
    /// A plain sentence that is safe to show to the user.
    pub message: String,
    /// The request field the problem concerns, when there is one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
}

impl ApiError {
    /// An error with no particular field.
    #[must_use]
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            field: None,
        }
    }

    /// An `invalid_request` error that concerns one request field.
    #[must_use]
    pub fn invalid(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self::for_field(ErrorCode::InvalidRequest, field, message)
    }

    /// An error that concerns one request field.
    #[must_use]
    pub fn for_field(
        code: ErrorCode,
        field: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code,
            message: message.into(),
            field: Some(field.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_serialise_as_snake_case() -> Result<(), serde_json::Error> {
        assert_eq!(
            serde_json::to_string(&ErrorCode::InvalidRequest)?,
            "\"invalid_request\""
        );
        assert_eq!(
            serde_json::to_string(&ErrorCode::UnsupportedMedia)?,
            "\"unsupported_media\""
        );
        Ok(())
    }

    #[test]
    fn error_round_trips_with_and_without_a_field() -> Result<(), serde_json::Error> {
        for original in [
            ApiError::new(ErrorCode::NotFound, "No such video."),
            ApiError::for_field(ErrorCode::InvalidRequest, "participant", "Required."),
        ] {
            let text = serde_json::to_string(&original)?;
            assert_eq!(serde_json::from_str::<ApiError>(&text)?, original);
        }
        Ok(())
    }

    #[test]
    fn absent_field_is_left_out_of_the_json() -> Result<(), serde_json::Error> {
        let text =
            serde_json::to_string(&ApiError::new(ErrorCode::Internal, "Something went wrong."))?;
        assert!(!text.contains("field"));
        Ok(())
    }

    #[test]
    fn a_body_without_a_field_still_parses() -> Result<(), serde_json::Error> {
        let parsed: ApiError = serde_json::from_str(r#"{"code":"conflict","message":"Exists."}"#)?;
        assert_eq!(parsed.field, None);
        Ok(())
    }
}
