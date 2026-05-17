//! Handlers for query and counterfactual endpoints.

use axum::extract::State;
use axum::Json;
use serde::Deserialize;
use std::sync::Arc;

use epica_core::CheckpointId;

use crate::{error::McpError, AppState};

#[derive(Debug, Deserialize)]
pub struct QueryRequest {
    pub query: String,
    pub budget_tokens: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct CounterfactualRequest {
    pub belief_key: String,
}

#[derive(Debug, Deserialize)]
pub struct DiffRequest {
    pub checkpoint_id: String,
}

pub async fn handle_belief_query(
    State(state): State<Arc<AppState>>,
    Json(req): Json<QueryRequest>,
) -> Result<Json<serde_json::Value>, McpError> {
    let budget = req.budget_tokens.unwrap_or(4096);
    let results = state.runtime.retrieve_for_query(&req.query, budget).await;
    let beliefs: Vec<serde_json::Value> = results
        .into_iter()
        .map(|s| {
            serde_json::json!({
                "key": s.key,
                "fast_confidence": s.fast_confidence,
                "slow_confidence": s.slow_confidence,
                "value": format!("{:?}", s.value),
            })
        })
        .collect();
    Ok(Json(serde_json::json!({ "beliefs": beliefs })))
}

pub async fn handle_counterfactual(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CounterfactualRequest>,
) -> Result<Json<serde_json::Value>, McpError> {
    let quad = state.runtime.read_quad().await;

    // Find belief by key
    let (id, _) = quad
        .iter()
        .find(|(_, n)| n.key == req.belief_key)
        .ok_or_else(|| McpError::NotFound { key: req.belief_key.clone() })?;

    let world = quad.counterfactual_query(id);
    let surviving: Vec<serde_json::Value> = world
        .surviving_beliefs()
        .map(|(_, n)| serde_json::json!({ "key": n.key, "fast_confidence": n.fast_confidence }))
        .collect();

    Ok(Json(serde_json::json!({
        "surviving": surviving,
        "excluded_count": world.excluded.len(),
    })))
}

pub async fn handle_diff(
    State(state): State<Arc<AppState>>,
    Json(req): Json<DiffRequest>,
) -> Result<Json<serde_json::Value>, McpError> {
    let cid = CheckpointId::parse(&req.checkpoint_id)
        .ok_or_else(|| McpError::BadRequest(format!("invalid checkpoint_id: {}", req.checkpoint_id)))?;

    let quad = state.runtime.read_quad().await;

    // Find the checkpoint by ID
    let checkpoint_quad = quad
        .checkpoint_by_id(cid)
        .map(|c| c.quad.as_ref())
        .ok_or_else(|| McpError::BadRequest(format!("checkpoint not found: {cid}")))?;

    let diff = quad.diff(checkpoint_quad);
    Ok(Json(serde_json::json!({
        "added": diff.added.len(),
        "removed": diff.removed.len(),
        "modified": diff.modified.len(),
        "trajectory_ece": diff.trajectory_ece,
    })))
}
