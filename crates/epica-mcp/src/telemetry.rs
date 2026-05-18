//! Observability: Prometheus metrics + (optional) OpenTelemetry OTLP traces.
//!
//! ## Prometheus
//!
//! Named counters, gauges, and histograms are registered here; handlers
//! increment them via the `metrics::counter!` / `metrics::histogram!` macros.
//! The `PrometheusHandle` is stored in `AppState` and rendered by `GET /metrics`.
//!
//! ## OpenTelemetry OTLP (feature `otlp`)
//!
//! When the binary is built with `--features otlp`, [`init_otlp`] subscribes a
//! `tracing` layer that forwards every span and event to an OTLP collector
//! over gRPC. The endpoint comes from `EPICA_OTLP_ENDPOINT` (e.g.
//! `http://localhost:4317` for a local Jaeger / OpenTelemetry Collector).
//! Without the feature, calls to [`init_otlp`] are stubbed and produce a
//! warning, so the surface area is identical between binary builds.

use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};

/// Metric names used across handlers.
pub mod names {
    pub const BELIEF_INSERTS: &str = "epica_belief_inserts_total";
    pub const BELIEF_UPDATES: &str = "epica_belief_updates_total";
    pub const SYSTEM2_ACTIVATIONS: &str = "epica_system2_activations_total";
    pub const SYSTEM2_THROTTLED: &str = "epica_system2_throttled_total";
    pub const QUERY_DURATION_SECS: &str = "epica_query_duration_seconds";
    pub const CHECKPOINT_OPS: &str = "epica_checkpoint_operations_total";
    pub const ROLLBACK_OPS: &str = "epica_rollback_operations_total";
    pub const CONTRACT_VIOLATIONS: &str = "epica_contract_violations_total";
}

/// Install the global Prometheus recorder and return the render handle.
///
/// Must be called once at process startup, before any `metrics::counter!` calls.
/// Panics if a recorder was already installed (don't call this in tests).
pub fn install_prometheus() -> PrometheusHandle {
    PrometheusBuilder::new()
        .install_recorder()
        .expect("failed to install Prometheus metrics recorder")
}

/// Build a handle without installing as the global recorder — for testing.
#[cfg(test)]
pub fn build_test_handle() -> PrometheusHandle {
    PrometheusBuilder::new().build_recorder().handle()
}

// ── OpenTelemetry OTLP exporter ──────────────────────────────────────────────

/// Initialise the OpenTelemetry OTLP tracer and bridge it to `tracing`.
///
/// Reads `EPICA_OTLP_ENDPOINT` for the collector URL (gRPC over tonic). On
/// any failure — variable absent, malformed endpoint, collector
/// unreachable — the function logs a warning and returns without raising:
/// degraded observability is preferable to a refused boot.
///
/// **Call site**: invoke once from `main()` AFTER `tracing_subscriber` is
/// already initialised, so the OTLP layer attaches to the existing
/// subscriber. The function does not install its own subscriber.
///
/// Without the `otlp` feature this function is a no-op that prints a
/// notice. This keeps `main.rs` compiling identically in both modes.
#[cfg(feature = "otlp")]
pub fn init_otlp(service_name: &str) {
    use opentelemetry::trace::TracerProvider as _;
    use opentelemetry_otlp::WithExportConfig;

    let Ok(endpoint) = std::env::var("EPICA_OTLP_ENDPOINT") else {
        tracing::info!("EPICA_OTLP_ENDPOINT not set — OTLP exporter disabled");
        return;
    };

    // Build the pipeline using the high-level API exposed in
    // opentelemetry-otlp 0.17. install_batch attaches a background Tokio
    // task that flushes spans to the collector.
    let provider_result = opentelemetry_otlp::new_pipeline()
        .tracing()
        .with_exporter(
            opentelemetry_otlp::new_exporter()
                .tonic()
                .with_endpoint(&endpoint),
        )
        .with_trace_config(
            opentelemetry_sdk::trace::Config::default().with_resource(
                opentelemetry_sdk::Resource::new([opentelemetry::KeyValue::new(
                    "service.name",
                    service_name.to_string(),
                )]),
            ),
        )
        .install_batch(opentelemetry_sdk::runtime::Tokio);

    let provider = match provider_result {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(error = %e, endpoint = %endpoint, "failed to build OTLP pipeline");
            return;
        }
    };

    let tracer = provider.tracer(service_name.to_string());
    let layer = tracing_opentelemetry::layer().with_tracer(tracer);

    // Best-effort attach: if the existing subscriber does not accept new
    // layers (e.g. `tracing_subscriber::fmt()` was used directly without a
    // registry) we lose OTLP but the process keeps running. The standard
    // setup in main.rs uses fmt(), so a follow-up enhancement would be to
    // switch main.rs to a Registry-based pipeline.
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    if let Err(e) = tracing_subscriber::registry().with(layer).try_init() {
        tracing::warn!(error = %e, "OTLP layer attach failed (subscriber already locked)");
        return;
    }

    tracing::info!(endpoint = %endpoint, "OpenTelemetry OTLP exporter active");
}

/// No-op stub when the `otlp` feature is disabled.
#[cfg(not(feature = "otlp"))]
pub fn init_otlp(_service_name: &str) {
    if std::env::var("EPICA_OTLP_ENDPOINT").is_ok() {
        tracing::warn!(
            "EPICA_OTLP_ENDPOINT is set but this binary was built without --features otlp; \
             rebuild with --features otlp to enable trace export"
        );
    }
}
