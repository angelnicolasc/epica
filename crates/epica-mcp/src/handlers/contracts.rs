//! Handlers for contract status endpoints.

use axum::extract::State;
use axum::Json;
use std::sync::Arc;

use crate::{error::McpError, AppState};

pub async fn handle_contract_status(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, McpError> {
    match &state.contract {
        Some(contract) => {
            let bound = contract.drift_bound();
            Ok(Json(serde_json::json!({
                "alpha": bound.alpha,
                "gamma": bound.gamma,
                "expected_drift": bound.expected_drift,
                "gaussian_concentration": bound.gaussian_concentration,
                "satisfies_delta_1_k_100": bound.satisfies(1, 100),
            })))
        }
        None => Ok(Json(serde_json::json!({ "status": "no contract configured" }))),
    }
}
