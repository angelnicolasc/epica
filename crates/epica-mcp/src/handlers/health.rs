//! Liveness and readiness probe handlers.

use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde::Serialize;
use std::sync::Arc;

use crate::AppState;

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub version: &'static str,
}

#[derive(Serialize)]
pub struct ReadyResponse {
    pub status: &'static str,
    pub beliefs: usize,
}

/// `GET /health` — liveness probe. Always 200 if the process is up.
pub async fn handle_health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
    })
}

/// `GET /ready` — readiness probe. Checks the runtime is operational.
///
/// Returns 200 with current belief count when ready, 503 if the runtime lock is
/// poisoned or unavailable.
pub async fn handle_ready(
    State(state): State<Arc<AppState>>,
) -> Result<Json<ReadyResponse>, StatusCode> {
    let quad = state.runtime.read_quad().await;
    let beliefs = quad.iter().count();
    Ok(Json(ReadyResponse {
        status: "ready",
        beliefs,
    }))
}

/// `GET /metrics` — Prometheus text exposition format.
pub async fn handle_metrics(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let body = state
        .prometheus
        .as_ref()
        .map(|h| h.render())
        .unwrap_or_default();

    (
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        body,
    )
}
