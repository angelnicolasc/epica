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
        // EPICA_NO_AUTH=1 is only permitted when EPICA_ENV is explicitly dev/test.
        // A missing EPICA_ENV is treated as production: a misconfigured deploy that
        // forgets to set env vars must fail loud, not silently expose an open server.
        let env_name = env_or("EPICA_ENV", "");
        let in_dev = matches!(
            env_name.to_lowercase().as_str(),
            "development" | "dev" | "test"
        );
        if !in_dev {
            eprintln!(
                "FATAL: EPICA_NO_AUTH=1 is only permitted when \
                 EPICA_ENV=development|dev|test.\n\
                 Current EPICA_ENV={env_name:?}. \
                 Set EPICA_ENV=development or remove EPICA_NO_AUTH."
            );
            std::process::exit(1);
        }
        tracing::warn!(
            env = %env_name,
            "⚠️  Auth disabled — development mode only (EPICA_ENV={env_name})"
        );
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
    // τ (tau): System 2 activation threshold. See DEVLOG HD-E4 for calibration plan.
    let reflection_threshold: f32 = env_f32("EPICA_REFLECTION_THRESHOLD", 0.15);

    // ── Optional OTLP endpoint (OpenTelemetry trace export) ────────────────────
    // When the binary is built with `--features otlp`, init_otlp() reads
    // EPICA_OTLP_ENDPOINT and attaches a tracing layer that forwards every
    // span (including those emitted by tower-http's TraceLayer) to the
    // collector over gRPC. Without the feature this is a no-op; setting
    // EPICA_OTLP_ENDPOINT without the feature prints an informative warning.
    epica_mcp::telemetry::init_otlp("epica-mcp");

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
    let mut runtime = BeliefRuntime::new(
        BeliefQuad::new(),
        reliability_baseline,
        system2_budget,
        system2_refill_rate,
    )
    .with_default_tau(reflection_threshold);

    // ── LLM provider selection (System 2 reflection) ──────────────────────────
    // `EPICA_LLM_PROVIDER` selects which client gets attached:
    //   - "anthropic" (default when the `anthropic` feature is on)
    //   - "openai"    (requires the `openai` feature)
    //   - "none"      (no client; System 2 returns Pending and tasks never advance)
    //
    // Missing API keys are treated as configuration errors: log loud and run
    // without System 2 rather than panicking, so the rest of the server stays
    // up. This is intentional — degraded mode is observable via /health and
    // the system2_throttled metric, while a hard crash is not.
    let provider = env_or("EPICA_LLM_PROVIDER", default_provider());
    match provider.to_lowercase().as_str() {
        "none" => {
            tracing::info!("LLM provider: none (System 2 reflection disabled)");
        }
        #[cfg(feature = "anthropic")]
        "anthropic" => match epica_anthropic::AnthropicLlmClient::from_env() {
            Ok(client) => {
                runtime = runtime.with_llm_client(Arc::new(client));
                tracing::info!("LLM provider: anthropic (Messages API)");
            }
            Err(e) => tracing::warn!(error = %e, "ANTHROPIC_API_KEY missing — System 2 disabled"),
        },
        #[cfg(feature = "openai")]
        "openai" => match epica_openai::OpenAiLlmClient::from_env() {
            Ok(client) => {
                runtime = runtime.with_llm_client(Arc::new(client));
                tracing::info!("LLM provider: openai (Chat Completions API, gpt-4o-mini)");
            }
            Err(e) => tracing::warn!(error = %e, "OPENAI_API_KEY missing — System 2 disabled"),
        },
        other => {
            tracing::warn!(
                provider = %other,
                "unknown or unavailable LLM provider — System 2 disabled. \
                 Rebuild with --features openai or --features anthropic to enable."
            );
        }
    }

    let runtime = Arc::new(runtime);

    tracing::info!(
        addr = %addr,
        no_auth = no_auth,
        system2_budget = system2_budget,
        reliability_baseline = reliability_baseline,
        llm_provider = %provider,
        "starting epica-serve v{}",
        env!("CARGO_PKG_VERSION")
    );

    epica_mcp::serve_blocking(runtime, addr, auth, contract);
}

/// Compile-time default for `EPICA_LLM_PROVIDER`. Falls back to "none" when
/// no provider feature is enabled so the binary still serves without an LLM.
fn default_provider() -> &'static str {
    #[cfg(feature = "anthropic")]
    {
        "anthropic"
    }
    #[cfg(all(not(feature = "anthropic"), feature = "openai"))]
    {
        "openai"
    }
    #[cfg(not(any(feature = "anthropic", feature = "openai")))]
    {
        "none"
    }
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
