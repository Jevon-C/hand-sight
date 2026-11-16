//! Endpoints that describe the installed tests.

use std::sync::Arc;

use axonhand_api::{TestDetail, TestList};
use axum::extract::{Path, State};
use axum::Json;

use crate::error::ServerError;
use crate::registry::Registry;

/// `GET /api/tests`
pub async fn list(State(registry): State<Arc<Registry>>) -> Json<TestList> {
    Json(registry.list())
}

/// `GET /api/tests/{id}`
///
/// # Errors
///
/// `not_found` when no plug-in folder has that id.
pub async fn get_one(
    State(registry): State<Arc<Registry>>,
    Path(id): Path<String>,
) -> Result<Json<TestDetail>, ServerError> {
    registry
        .detail(&id)
        .cloned()
        .map(Json)
        .ok_or_else(|| ServerError::NotFound("No such test.".to_owned()))
}
