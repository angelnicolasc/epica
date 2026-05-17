//! Handlers for checkpoint and rollback endpoints.

use axum::extract::State;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use epica_core::CheckpointId;

use crate::{error::McpError, AppState};

#[derive(Debug, Serialize)]
pub struct CheckpointResponse {
    pub checkpoint_id: String,
    pub version: u64,
}

#[derive(Debug, Deserialize)]
pub struct RollbackRequest {
    pub checkpoint_id: String,
}

pub async fn handle_checkpoint(
    State(state): State<Arc<AppState>>,
) -> Result<Json<CheckpointResponse>, McpError> {
    let checkpoint_id = state.runtime.checkpoint().await;
    let version = state.runtime.read_quad().await.version();
    Ok(Json(CheckpointResponse {
        checkpoint_id: checkpoint_id.to_string(),
        version,
    }))
}

pub async fn handle_rollback(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RollbackRequest>,
) -> Result<Json<serde_json::Value>, McpError> {
    let cid = CheckpointId::parse(&req.checkpoint_id)
        .ok_or_else(|| McpError::BadRequest(format!("invalid checkpoint_id: {}", req.checkpoint_id)))?;

    let diff = state
        .runtime
        .rollback_to(cid)
        .await
        .map_err(|e| McpError::Runtime(e.to_string()))?;

    Ok(Json(serde_json::json!({
        "added": diff.added.len(),
        "removed": diff.removed.len(),
        "modified": diff.modified.len(),
        "trajectory_ece": diff.trajectory_ece,
    })))
}
