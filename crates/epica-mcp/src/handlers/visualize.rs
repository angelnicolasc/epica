//! Visualisation endpoint — exposes the live `BeliefQuad` as Graphviz DOT.
//!
//! `GET /v1/visualize/dot` returns the current state of the runtime's quad
//! serialised as a DOT document with `Content-Type: text/vnd.graphviz`.
//! The caller can pipe it through `dot -Tsvg` to render an SVG.
//!
//! There is no rendering on the server side: producing an image would force
//! Graphviz as a runtime dependency, and DOT is the universal lingua franca
//! for graph layout. The MCP host can choose to render however it likes.

use std::sync::Arc;

use axum::{
    extract::State,
    http::{header, StatusCode},
    response::IntoResponse,
};

use crate::AppState;

/// `GET /v1/visualize/dot` — current `BeliefQuad` as Graphviz DOT.
pub async fn handle_visualize_dot(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let quad = state.runtime.read_quad().await;
    let dot = epica_core::quad::viz::to_dot(&quad);
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/vnd.graphviz; charset=utf-8")],
        dot,
    )
}
