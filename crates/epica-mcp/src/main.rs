//! `epica-serve` — MCP 2026 server entry point.
//!
//! All configuration via environment variables; use `--` flags or env vars:
//!
//! ```
//! epica-serve \
//!   --addr 0.0.0.0:8080 \
//!   --no-auth \
//!   --otlp-endpoint http://localhost:4317
//! ```
//!
//! Or equivalently with env vars:
//! ```
//! EPICA_ADDR=0.0.0.0:8080 EPICA_NO_AUTH=1 epica-serve
//! ```

use std::{net::SocketAddr, sync::Arc};

use epica_contracts::{BehavioralContract, ContractConfig};
use epica_core::BeliefQuad;
use epica_mcp::AuthConfig;
use epica_runtime::BeliefRuntime;

fn main() {
    // ── Logging ────────────────────────────────────────────────────────────────
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("EPICA_LOG")
                .unwrap_or_else(|_| "epica_mcp=info,tower_http=debug".into()),
        )
        .with_target(false)
        .compact()
        .init();

    // ── Network ────────────────────────────────────────────────────────────────
    let addr: SocketAddr = env_or("EPICA_ADDR", "127.0.0.1:8765")
        .parse()
        .expect("EPICA_ADDR must be a valid socket address (e.g. 0.0.0.0:8080)");

    // ── Auth ───────────────────────────────────────────────────────────────────
    let no_auth = env_flag("EPICA_NO_AUTH");
    let auth = if no_auth {
        tracing::warn!("⚠️  Auth disabled via --no-auth / EPICA_NO_AUTH=1 — development mode only");
        AuthConfig::disabled()
    } else {
        AuthConfig::from_env()
    };

    // ── Runtime parameters ─────────────────────────────────────────────────────
    // `reliability_baseline` is `b` in the AUQ paper §3.2 — the expected accuracy
    // of the model on the current task distribution. Default 0.5 is deliberately
    // conservative; tune per deployment.
    let reliability_baseline: f32 = env_f32("EPICA_RELIABILITY", 0.5);
    let system2_budget: u32 = env_u32("EPICA_SYSTEM2_BUDGET", 50);
    let system2_refill_rate: f32 = env_f32("EPICA_SYSTEM2_REFILL", 1.0);

    // ── Optional OTLP endpoint (OpenTelemetry trace export) ────────────────────
    // If set, configure tracing-opentelemetry + opentelemetry-otlp in your process
    // wrapper. The TraceLayer in server.rs emits tracing spans automatically.
    if let Ok(otlp) = std::env::var("EPICA_OTLP_ENDPOINT") {
        tracing::info!("OTLP endpoint configured: {otlp} (configure opentelemetry-otlp subscriber to export)");
    }

    // ── Behavioral contract (optional, loaded from TOML) ───────────────────────
    // Set EPICA_CONTRACTS_FILE=/path/to/contract.toml to enable governance.
    // Without this, the runtime runs without behavioral contract enforcement.
    let contract: Option<BehavioralContract> =
        std::env::var("EPICA_CONTRACTS_FILE").ok().map(|path| {
            let toml_str = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("failed to read EPICA_CONTRACTS_FILE={path}: {e}"));
            let cfg: ContractConfig = toml::from_str(&toml_str)
                .unwrap_or_else(|e| panic!("failed to parse contract TOML at {path}: {e}"));
            tracing::info!(domain = %cfg.domain, "loaded behavioral contract from {path}");
            BehavioralContract::from_config(cfg)
        });

    // ── Runtime ────────────────────────────────────────────────────────────────
    let runtime = Arc::new(BeliefRuntime::new(
        BeliefQuad::new(),
        reliability_baseline,
        system2_budget,
        system2_refill_rate,
    ));

    tracing::info!(
        addr = %addr,
        no_auth = no_auth,
        system2_budget = system2_budget,
        reliability_baseline = reliability_baseline,
        "starting epica-serve v{}",
        env!("CARGO_PKG_VERSION")
    );

    epica_mcp::serve_blocking(runtime, addr, auth, contract);
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn env_flag(key: &str) -> bool {
    std::env::var(key)
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

fn env_f32(key: &str, default: f32) -> f32 {
    std::env::var(key)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default)
}

fn env_u32(key: &str, default: u32) -> u32 {
    std::env::var(key)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default)
}
