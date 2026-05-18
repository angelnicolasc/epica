//! MCP 2026 HTTP server — Axum router with full middleware stack.
//!
//! Middleware order (outermost → innermost, i.e. first to see each request):
//!   1. Rate limit — per-IP token-bucket (governor), configured via EPICA_RATE_LIMIT_RPS
//!   2. Auth       — OAuth 2.1 JWT Bearer (from_fn closure, skips exempt paths)
//!   3. CORS       — permissive in dev, restricted via EPICA_CORS_ORIGINS in prod
//!   4. Trace      — tower-http TraceLayer, OTel-compatible spans

use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    extract::Request,
    middleware::{self, Next},
    routing::{get, post},
    Json, Router,
};
use tower_http::trace::TraceLayer;

use epica_runtime::BeliefRuntime;

use crate::{
    auth::{check_auth, AuthConfig},
    handlers::{
        belief::{handle_belief_get, handle_belief_set, handle_belief_set_result},
        checkpoint::{handle_checkpoint, handle_rollback},
        contracts::handle_contract_status,
        health::{handle_health, handle_metrics, handle_ready},
        query::{handle_belief_query, handle_counterfactual, handle_diff},
        task_stream::handle_task_stream,
        visualize::handle_visualize_dot,
    },
    middleware::{build_rate_limiter, cors_layer, rate_limit_middleware},
    server_card::McpServerCard,
    tasks::task_store_from_env,
    AppState,
};

/// Build the Axum router for the given shared state.
///
/// Extracted from `serve_blocking` so tests can call it without binding a port.
pub fn build_router(state: Arc<AppState>) -> Router {
    let auth_cfg = state.auth.clone();
    let rate_limiter = state.rate_limiter.clone();

    Router::new()
        // ── Infrastructure ─────────────────────────────────────────────────────
        .route("/health", get(handle_health))
        .route("/ready", get(handle_ready))
        .route("/metrics", get(handle_metrics))
        // ── Discovery (exempt from auth — check_auth skips /.well-known/*) ─────
        .route(
            "/.well-known/epica-server-card.json",
            get(|| async { Json(McpServerCard::default_card()) }),
        )
        .route(
            "/.well-known/jwks.json",
            get(handle_jwks),
        )
        // ── Belief CRUD ─────────────────────────────────────────────────────────
        .route("/v1/beliefs/:key", get(handle_belief_get))
        .route("/v1/beliefs", post(handle_belief_set))
        // ── SEP-1686 Tasks (poll + SSE stream) ──────────────────────────────────
        .route("/v1/tasks/:id", get(handle_belief_set_result))
        .route("/v1/tasks/:id/stream", get(handle_task_stream))
        // ── Checkpoint / rollback ────────────────────────────────────────────────
        .route("/v1/checkpoint", post(handle_checkpoint))
        .route("/v1/rollback", post(handle_rollback))
        // ── Contract governance ──────────────────────────────────────────────────
        .route("/v1/contract/status", get(handle_contract_status))
        // ── Query / analysis ─────────────────────────────────────────────────────
        .route("/v1/query", post(handle_belief_query))
        .route("/v1/counterfactual", post(handle_counterfactual))
        .route("/v1/diff", post(handle_diff))
        // ── Visualisation ────────────────────────────────────────────────────────
        .route("/v1/visualize/dot", get(handle_visualize_dot))
        // ── State ────────────────────────────────────────────────────────────────
        .with_state(state)
        // ── Middleware stack — layers applied inside-out, so LAST = OUTERMOST ───
        // Rate limit: outermost — applied before auth, protects all non-exempt paths.
        .layer(middleware::from_fn(move |req: Request, next: Next| {
            let rl = rate_limiter.clone();
            async move { rate_limit_middleware(rl, req, next).await }
        }))
        // Auth: JWT Bearer validation, exempt paths skipped.
        .layer(middleware::from_fn(move |mut req: Request, next: Next| {
            let cfg = auth_cfg.clone();
            async move {
                req.extensions_mut().insert(cfg.clone());
                match check_auth(&req, &cfg) {
                    Ok(()) => next.run(req).await,
                    Err(resp) => resp,
                }
            }
        }))
        .layer(cors_layer())
        .layer(TraceLayer::new_for_http())
}

/// JWKS handler — delegates to AuthConfig for mode-aware key publication.
async fn handle_jwks(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    Json(state.auth.jwks_response())
}

/// Start the MCP server and block until shutdown.
///
/// Configuration via environment variables:
/// - `EPICA_RATE_LIMIT_RPS`    — requests per second per IP (default 100)
/// - `EPICA_CONTRACTS_FILE`    — path to a TOML contract file (optional)
pub fn serve_blocking(
    runtime: Arc<BeliefRuntime>,
    addr: SocketAddr,
    auth: AuthConfig,
    contract: Option<epica_contracts::BehavioralContract>,
) {
    let prometheus = crate::telemetry::install_prometheus();

    let rps: u32 = std::env::var("EPICA_RATE_LIMIT_RPS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(100);

    let rt = tokio::runtime::Runtime::new().expect("failed to build tokio runtime");
    rt.block_on(async {
        let state = Arc::new(AppState {
            runtime,
            task_store: task_store_from_env(),
            contract,
            auth,
            rate_limiter: build_rate_limiter(rps),
            prometheus: Some(prometheus),
        });

        let app = build_router(state);

        tracing::info!("epica-serve listening on {addr}");
        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .expect("failed to bind");
        axum::serve(listener, app).await.expect("server error");
    });
}
