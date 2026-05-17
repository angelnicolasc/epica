//! Observability: Prometheus metrics via the `metrics` ecosystem.
//!
//! Named counters, gauges, and histograms are registered here; handlers increment
//! them via the `metrics::counter!` / `metrics::histogram!` macros. The
//! `PrometheusHandle` is stored in `AppState` and rendered by `GET /metrics`.
//!
//! For OpenTelemetry trace export (OTLP), configure `tracing-opentelemetry` with
//! `opentelemetry-otlp` in the process entry point — the existing `tower-http`
//! `TraceLayer` already emits tracing spans that bridge automatically.

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
    PrometheusBuilder::new()
        .build_recorder()
        .handle()
}
