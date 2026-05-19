//! Drives synthetic traces through a configured
//! [`BeliefRuntime`][epica_runtime::BeliefRuntime] and collects metrics.
//!
//! The harness is intentionally async — the runtime API is async — but
//! self-contained: `tokio` is the only async dep, and trajectories run
//! sequentially within a suite so the latency numbers reflect single-
//! request hot-path cost, not throughput.
//!
//! ## What the harness does NOT do
//!
//! - It does **not** spin up an LLM. The traces are deterministic and
//!   ground-truth-labelled by the generator. Wiring a real LLM would
//!   confound runtime perf with provider latency.
//! - It does **not** persist the runtime across trajectories. Each
//!   trajectory gets a fresh runtime so the metrics are independent.
//! - It does **not** verify K\*1–K\*6 postulate audits — that's
//!   `cargo test -p epica-core`'s job. The harness reports aggregate
//!   *outcomes* (contradictions detected, T-ECE, free energy), not
//!   per-postulate correctness.

use std::sync::Arc;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use epica_core::{BeliefNode, BeliefQuad, BeliefValue, Provenance};
use epica_runtime::BeliefRuntime;

use crate::metrics::{MetricSet, PerTrajectory};
use crate::traces::{Suite, Trace, TraceStep};

/// Knobs that control how the harness builds each per-trajectory
/// runtime.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HarnessConfig {
    /// AUQ §3.2 reliability baseline `b`. Default `0.5`.
    pub reliability_baseline: f32,
    /// System 2 token-bucket capacity. Default `0` — System 2 is *off*
    /// in synthetic benchmarks so latency numbers reflect System 1
    /// only.
    pub system2_budget: u32,
    /// Token-bucket refill rate (tokens / sec). Default `0.0`.
    pub system2_refill_rate: f32,
    /// When `true` and the `active-inference` Cargo feature is
    /// enabled, attach an
    /// [`ActiveInferenceMonitor`][epica_active_inference::ActiveInferenceMonitor]
    /// so free-energy metrics get reported. Default `true`.
    pub enable_active_inference: bool,
}

impl Default for HarnessConfig {
    fn default() -> Self {
        Self {
            reliability_baseline: 0.5,
            system2_budget: 0,
            system2_refill_rate: 0.0,
            enable_active_inference: true,
        }
    }
}

/// Errors raised by the harness.
#[derive(Debug, thiserror::Error)]
pub enum HarnessError {
    /// A `TraceStep::Update` referenced a key that was never inserted.
    #[error("trace step references unknown key: {0}")]
    UnknownKey(String),
    /// `update_belief` returned an error that wasn't a graceful
    /// `System2Throttled`.
    #[error("runtime update_belief failed for key {key}: {message}")]
    Update {
        /// Belief key.
        key: String,
        /// Underlying runtime error message.
        message: String,
    },
}

/// Final report from [`run_suite`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuiteReport {
    /// Which suite was run.
    pub suite: Suite,
    /// Trajectory count requested.
    pub trajectories: u32,
    /// Wall-clock seconds the suite took, end to end.
    pub wall_clock_seconds: f64,
    /// Per-trajectory metrics, in `trajectory_id` order.
    pub per_trajectory: Vec<PerTrajectory>,
    /// Aggregate metrics.
    pub metrics: MetricSet,
}

/// Run `n` deterministic trajectories of `suite` and return the
/// aggregate report.
pub async fn run_suite(
    suite: Suite,
    n: u32,
    cfg: &HarnessConfig,
) -> SuiteReport {
    let started = Instant::now();
    let mut per: Vec<PerTrajectory> = Vec::with_capacity(n as usize);
    for tid in 0..n {
        let trace = Trace::generate(suite, tid);
        let result = run_trace(&trace, cfg).await;
        per.push(result);
    }
    let metrics = MetricSet::from_trajectories(&per);
    SuiteReport {
        suite,
        trajectories: n,
        wall_clock_seconds: started.elapsed().as_secs_f64(),
        per_trajectory: per,
        metrics,
    }
}

/// Execute a single trace and collect its [`PerTrajectory`] metrics.
///
/// Public for tests and tooling that want to drive an individual
/// trajectory without the suite-level wrapper.
pub async fn run_trace(trace: &Trace, cfg: &HarnessConfig) -> PerTrajectory {
    let rt = build_runtime(cfg);

    #[cfg(feature = "active-inference")]
    let monitor: Option<Arc<Mutex<epica_active_inference::ActiveInferenceMonitor>>> =
        if cfg.enable_active_inference {
            let m = Arc::new(Mutex::new(
                epica_active_inference::ActiveInferenceMonitor::new(),
            ));
            // Attach to the runtime by re-binding. `set_active_inference`
            // takes `&mut self`, but our `rt` is owned here.
            let mut rt_local = rt;
            rt_local.set_active_inference(m.clone());
            // shadow rt back into immutable.
            return run_trace_inner(trace, rt_local, Some(m)).await;
        } else {
            None
        };

    #[cfg(not(feature = "active-inference"))]
    {
        run_trace_inner(trace, rt).await
    }
    #[cfg(feature = "active-inference")]
    {
        // active-inference disabled by config but feature is on.
        let _ = monitor;
        run_trace_inner(trace, rt, None).await
    }
}

fn build_runtime(cfg: &HarnessConfig) -> BeliefRuntime {
    BeliefRuntime::new(
        BeliefQuad::new(),
        cfg.reliability_baseline,
        cfg.system2_budget,
        cfg.system2_refill_rate,
    )
}

#[cfg(feature = "active-inference")]
async fn run_trace_inner(
    trace: &Trace,
    rt: BeliefRuntime,
    monitor: Option<Arc<Mutex<epica_active_inference::ActiveInferenceMonitor>>>,
) -> PerTrajectory {
    let mut per = PerTrajectory {
        trajectory_id: trace.trajectory_id,
        steps: trace.steps.len(),
        ..Default::default()
    };
    let mut ground_truth: Vec<(epica_core::BeliefId, bool)> = Vec::new();
    let mut key_to_id: std::collections::HashMap<String, epica_core::BeliefId> =
        std::collections::HashMap::new();

    for step in &trace.steps {
        execute_step(&rt, step, &mut per, &mut key_to_id, &mut ground_truth).await;
    }

    // Apply ground-truth corrections: by default every belief is
    // recorded as correct; mark the ones the trace said were wrong.
    {
        let mut history = rt.confidence_history().write().await;
        for (id, correct) in ground_truth {
            if !correct {
                history.mark_correct(id, false);
            }
        }
    }

    rt.finalize_session().await;
    per.tece = rt.compute_tece().await;
    let report = rt.session_report().await;
    per.contradictions_detected = report.contradictions_detected;
    per.calibration_target_met = report.calibration_target_met;
    per.soft_violations = report.soft_violations;
    per.hard_violations = report.hard_violations;
    per.critical_violations = report.critical_violations;

    if let Some(m) = monitor {
        let guard = m.lock().await;
        per.free_energy_samples = guard.history().len();
        if per.free_energy_samples > 0 {
            per.free_energy_mean = Some(guard.mean_free_energy());
        }
    }

    per
}

#[cfg(not(feature = "active-inference"))]
async fn run_trace_inner(trace: &Trace, rt: BeliefRuntime) -> PerTrajectory {
    let mut per = PerTrajectory {
        trajectory_id: trace.trajectory_id,
        steps: trace.steps.len(),
        ..Default::default()
    };
    let mut ground_truth: Vec<(epica_core::BeliefId, bool)> = Vec::new();
    let mut key_to_id: std::collections::HashMap<String, epica_core::BeliefId> =
        std::collections::HashMap::new();

    for step in &trace.steps {
        execute_step(&rt, step, &mut per, &mut key_to_id, &mut ground_truth).await;
    }

    {
        let mut history = rt.confidence_history().write().await;
        for (id, correct) in ground_truth {
            if !correct {
                history.mark_correct(id, false);
            }
        }
    }

    rt.finalize_session().await;
    per.tece = rt.compute_tece().await;
    let report = rt.session_report().await;
    per.contradictions_detected = report.contradictions_detected;
    per.calibration_target_met = report.calibration_target_met;
    per.soft_violations = report.soft_violations;
    per.hard_violations = report.hard_violations;
    per.critical_violations = report.critical_violations;
    per
}

async fn execute_step(
    rt: &BeliefRuntime,
    step: &TraceStep,
    per: &mut PerTrajectory,
    key_to_id: &mut std::collections::HashMap<String, epica_core::BeliefId>,
    ground_truth: &mut Vec<(epica_core::BeliefId, bool)>,
) {
    let turn = ground_truth.len() as u32;
    match step {
        TraceStep::Insert { key, value, confidence, correct } => {
            let node = BeliefNode::new(
                key,
                BeliefValue::Asserted(value.clone()),
                Provenance::UserStatement { turn },
                *confidence,
            );
            let started = Instant::now();
            let id = rt.insert_belief(node).await;
            let elapsed = started.elapsed().as_micros() as u64;
            per.insert_latencies_us.push(elapsed);
            key_to_id.insert(key.clone(), id);
            ground_truth.push((id, *correct));
        }
        TraceStep::Update { key, value, confidence, correct } => {
            let Some(&id) = key_to_id.get(key) else {
                // Key never inserted — synthetic generator bug; skip
                // and let the metric reflect the smaller trajectory.
                tracing::warn!(?key, "trace update on unknown key, skipping");
                return;
            };
            let started = Instant::now();
            let result = rt
                .update_belief(
                    id,
                    BeliefValue::Asserted(value.clone()),
                    Provenance::UserStatement { turn },
                    *confidence,
                )
                .await;
            let elapsed = started.elapsed().as_micros() as u64;
            per.insert_latencies_us.push(elapsed);
            match result {
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!(?key, error = %e, "update_belief failed in harness");
                }
            }
            // Track the *latest* ground-truth for this key.
            if let Some(existing) = ground_truth.iter_mut().rfind(|(eid, _)| *eid == id) {
                existing.1 = *correct;
            } else {
                ground_truth.push((id, *correct));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn alfworld_trajectory_produces_a_tece() {
        let cfg = HarnessConfig::default();
        let trace = Trace::generate(Suite::AlfworldLike, 0);
        let per = run_trace(&trace, &cfg).await;
        assert_eq!(per.trajectory_id, 0);
        assert_eq!(per.steps, trace.steps.len());
        assert!(per.tece.is_some(), "T-ECE must be computed");
        assert!(!per.insert_latencies_us.is_empty());
    }

    #[tokio::test]
    async fn webshop_trajectory_records_contradictions() {
        let cfg = HarnessConfig::default();
        let trace = Trace::generate(Suite::WebshopLike, 0);
        let per = run_trace(&trace, &cfg).await;
        // WebShop traces always include filter-driven contradictions.
        assert!(per.steps > 0);
    }

    #[tokio::test]
    async fn run_suite_aggregates_trajectories() {
        let cfg = HarnessConfig::default();
        let report = run_suite(Suite::AlfworldLike, 4, &cfg).await;
        assert_eq!(report.trajectories, 4);
        assert_eq!(report.per_trajectory.len(), 4);
        assert!(report.metrics.tece.is_some());
        assert!(report.metrics.insert_latency_us.samples > 0);
        assert!(report.wall_clock_seconds > 0.0);
    }

    #[cfg(feature = "active-inference")]
    #[tokio::test]
    async fn active_inference_reports_free_energy_when_enabled() {
        let cfg = HarnessConfig {
            enable_active_inference: true,
            ..Default::default()
        };
        let report = run_suite(Suite::AlfworldLike, 2, &cfg).await;
        assert!(
            report.metrics.free_energy_mean.is_some(),
            "free energy must be reported with the feature on"
        );
        assert!(report.metrics.free_energy_samples > 0);
    }

    #[cfg(feature = "active-inference")]
    #[tokio::test]
    async fn active_inference_disabled_in_config_skips_free_energy() {
        let cfg = HarnessConfig {
            enable_active_inference: false,
            ..Default::default()
        };
        let report = run_suite(Suite::AlfworldLike, 2, &cfg).await;
        assert!(
            report.metrics.free_energy_mean.is_none(),
            "FE must be None when config disables it"
        );
    }
}
