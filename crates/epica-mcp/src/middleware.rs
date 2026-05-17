//! Tower middleware layers: CORS and per-IP rate limiting.
//!
//! Rate limiting uses the `governor` crate (leaky-bucket / token-bucket) keyed
//! by client IP. Configured via `EPICA_RATE_LIMIT_RPS` (default: 100 req/s).
//!
//! IP extraction order: `X-Forwarded-For` header → falls back to `127.0.0.1`
//! when running behind a proxy that doesn't set the header (e.g. tests).

use std::{
    net::{IpAddr, Ipv4Addr},
    num::NonZeroU32,
    sync::Arc,
};

use axum::{
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use governor::{DefaultKeyedRateLimiter, Quota, RateLimiter};
use serde_json::json;
use tower_http::cors::{Any, CorsLayer};

// ── Rate limiter type ─────────────────────────────────────────────────────────

/// A per-IP keyed rate limiter backed by governor's token-bucket algorithm.
pub type IpRateLimiter = DefaultKeyedRateLimiter<IpAddr>;

/// Build a rate limiter that allows `requests_per_second` per unique IP.
///
/// A minimum of 1 RPS is enforced even if `requests_per_second` is 0.
pub fn build_rate_limiter(requests_per_second: u32) -> Arc<IpRateLimiter> {
    let rps = NonZeroU32::new(requests_per_second.max(1)).expect("rps is non-zero");
    Arc::new(RateLimiter::keyed(Quota::per_second(rps)))
}

// ── Rate limit middleware ─────────────────────────────────────────────────────

/// Axum middleware that enforces per-IP rate limits.
///
/// Exempt paths (infrastructure probes, never rate-limited):
/// - `/health`, `/ready`, `/metrics`, `/.well-known/*`
pub async fn rate_limit_middleware(
    limiter: Arc<IpRateLimiter>,
    req: Request,
    next: Next,
) -> Response {
    let path = req.uri().path();

    // Infrastructure probes bypass rate limiting — load balancers need these.
    let exempt = path == "/health"
        || path == "/ready"
        || path == "/metrics"
        || path.starts_with("/.well-known/");

    if exempt {
        return next.run(req).await;
    }

    let ip = extract_client_ip(&req);
    match limiter.check_key(&ip) {
        Ok(_) => next.run(req).await,
        Err(_) => (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({ "error": "rate limit exceeded — retry after 1 second" })),
        )
            .into_response(),
    }
}

/// Extract the client IP from `X-Forwarded-For` or fall back to localhost.
///
/// Takes the first address in the comma-separated `X-Forwarded-For` list,
/// which is the original client when the proxy appends itself.
fn extract_client_ip(req: &Request) -> IpAddr {
    req.headers()
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST))
}

// ── CORS ──────────────────────────────────────────────────────────────────────

/// Build a CORS layer.
///
/// In dev (no `EPICA_CORS_ORIGINS` set) → permissive (all origins allowed).
/// In production → restrict to the comma-separated list in `EPICA_CORS_ORIGINS`.
pub fn cors_layer() -> CorsLayer {
    if let Ok(origins_str) = std::env::var("EPICA_CORS_ORIGINS") {
        let valid_origins: Vec<axum::http::HeaderValue> = origins_str
            .split(',')
            .filter_map(|s| s.trim().parse().ok())
            .collect();

        if !valid_origins.is_empty() {
            return CorsLayer::new()
                .allow_origin(valid_origins)
                .allow_methods(Any)
                .allow_headers(Any);
        }
    }

    CorsLayer::permissive()
}
