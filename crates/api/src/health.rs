//! Health check response.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Overall state reported by the health check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum HealthStatus {
    /// The server is running and ready for requests.
    Ok,
}

/// Body of `GET /api/health`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct Health {
    /// Always `ok` while the server can answer.
    pub status: HealthStatus,
    /// Version of the server program.
    pub version: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_round_trips() -> Result<(), serde_json::Error> {
        let original = Health {
            status: HealthStatus::Ok,
            version: "0.1.0".to_owned(),
        };
        let text = serde_json::to_string(&original)?;
        assert_eq!(text, r#"{"status":"ok","version":"0.1.0"}"#);
        assert_eq!(serde_json::from_str::<Health>(&text)?, original);
        Ok(())
    }
}
