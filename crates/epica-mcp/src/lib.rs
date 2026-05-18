//! # epica-mcp
//!
//! MCP 2026 server for Epica — formal AGM belief revision runtime exposed as an
//! enterprise HTTP API with OAuth 2.1, SEP-1686 Tasks, Server Cards, and Prometheus.

use std::sync::Arc;

pub mod auth;
pub mod error;
pub mod handlers;
pub mod middleware;
pub mod server;
pub mod server_card;
pub mod tasks;
pub mod telemetry;

pub use auth::AuthConfig;
pub use error::McpError;
pub use server::{build_router, serve_blocking};
pub use server_card::McpServerCard;

/// Shared state threaded through all Axum handlers via `State<Arc<AppState>>`.
pub struct AppState {
    /// The live belief runtime (BeliefQuad + System 1 + System 2 + contracts).
    pub runtime: Arc<epica_runtime::BeliefRuntime>,
    /// SEP-1686 task store (in-memory or sled-backed) — System 2 call-now/fetch-later results.
    pub task_store: Box<dyn tasks::TaskStore>,
    /// Optional behavioral contract for the `/v1/contract/status` endpoint.
    pub contract: Option<epica_contracts::BehavioralContract>,
    /// OAuth 2.1 configuration — used by the auth middleware.
    pub auth: AuthConfig,
    /// Per-IP rate limiter (token-bucket, default 100 req/s via `EPICA_RATE_LIMIT_RPS`).
    pub rate_limiter: std::sync::Arc<middleware::IpRateLimiter>,
    /// Prometheus metrics handle — `None` in tests (no global recorder installed).
    pub prometheus: Option<metrics_exporter_prometheus::PrometheusHandle>,
}
