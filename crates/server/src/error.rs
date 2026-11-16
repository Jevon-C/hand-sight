//! Server errors and their mapping to the shared error response.

use axonhand_api::{ApiError, ErrorCode};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

/// An error from a request handler.
///
/// Each variant becomes one HTTP status and one [`ApiError`] body. Internal details are
/// logged but never sent to the client.
#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    /// The request was invalid; the message is safe to show to the user.
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    /// The requested item does not exist.
    #[error("not found: {0}")]
    NotFound(String),
    /// The upload is larger than the allowed size.
    #[error("too large: {0}")]
    TooLarge(String),
    /// The upload is not a readable video.
    #[error("unsupported media: {0}")]
    UnsupportedMedia(String),
    /// A request field is wrong; carries the shared error body unchanged.
    #[error("invalid field: {}", .0.message)]
    Field(ApiError),
    /// An unexpected failure. The text is for the log only.
    #[error("internal error: {0}")]
    Internal(String),
}

impl ServerError {
    fn parts(&self) -> (StatusCode, ApiError) {
        match self {
            Self::InvalidRequest(message) => (
                StatusCode::BAD_REQUEST,
                ApiError::new(ErrorCode::InvalidRequest, message.clone()),
            ),
            Self::NotFound(message) => (
                StatusCode::NOT_FOUND,
                ApiError::new(ErrorCode::NotFound, message.clone()),
            ),
            Self::TooLarge(message) => (
                StatusCode::PAYLOAD_TOO_LARGE,
                ApiError::new(ErrorCode::TooLarge, message.clone()),
            ),
            Self::UnsupportedMedia(message) => (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                ApiError::new(ErrorCode::UnsupportedMedia, message.clone()),
            ),
            Self::Field(error) => (StatusCode::BAD_REQUEST, error.clone()),
            Self::Internal(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                ApiError::new(ErrorCode::Internal, "Something went wrong on the server."),
            ),
        }
    }
}

impl IntoResponse for ServerError {
    fn into_response(self) -> Response {
        if let Self::Internal(detail) = &self {
            tracing::error!(%detail, "request failed");
        }
        let (status, body) = self.parts();
        (status, Json(body)).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_error_maps_to_its_status_and_code() {
        let cases = [
            (
                ServerError::InvalidRequest("bad".into()),
                StatusCode::BAD_REQUEST,
                ErrorCode::InvalidRequest,
            ),
            (
                ServerError::NotFound("gone".into()),
                StatusCode::NOT_FOUND,
                ErrorCode::NotFound,
            ),
            (
                ServerError::TooLarge("big".into()),
                StatusCode::PAYLOAD_TOO_LARGE,
                ErrorCode::TooLarge,
            ),
            (
                ServerError::UnsupportedMedia("odd".into()),
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                ErrorCode::UnsupportedMedia,
            ),
            (
                ServerError::Field(ApiError::invalid("hand", "Pick a hand.")),
                StatusCode::BAD_REQUEST,
                ErrorCode::InvalidRequest,
            ),
            (
                ServerError::Internal("boom".into()),
                StatusCode::INTERNAL_SERVER_ERROR,
                ErrorCode::Internal,
            ),
        ];
        for (error, status, code) in cases {
            let (got_status, body) = error.parts();
            assert_eq!(got_status, status);
            assert_eq!(body.code, code);
        }
    }

    #[test]
    fn internal_details_are_not_sent_to_the_client() {
        let (_, body) = ServerError::Internal("database path /secret".into()).parts();
        assert!(!body.message.contains("secret"));
    }
}
