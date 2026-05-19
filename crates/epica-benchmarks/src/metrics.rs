//! Metric aggregation for the harness.
//!
//! Five families:
//!
//! 1. **BeliefShift / T-ECE** — already computed inside `BeliefRuntime`
//!    via `compute_tece()`. We surface it per-trajectory and aggregate
//!    the mean across the suite.
//! 2. **Contract violations** — from `SessionReport`'s `soft_violations`
//!    / `hard_violations` / `critical_violations` counters.
//! 3. **Free-energy mean** — when the `active-inference` feature is
//!    enabled, average over the `ActiveInferenceMonitor`'s rolling
//!    history at end-of-trajectory.
//! 4. **Insert latency p50/p99** — measured in the harness; we keep
//!    every per-call duration in micros and compute the percentiles
//!    by sort+index. Cheap enough for our trajectory sizes.
//! 5. **Coverage counts** — total trajectories, total operations,
//!    AGM revisions actually triggered. Useful for sanity-checking
//!    the synthetic generators.

use serde::{Deserialize, Serialize};

/// Latency summary, in microseconds. All fields are 0 when no samples
/// were observed.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct LatencyStats {
    /// Number of samples.
    pub samples: usize,
    /// Arithmetic mean.
    pub mean: f64,
    /// 50th percentile.
    pub p50: u64,
    /// 95th percentile.
    pub p95: u64,
    /// 99th percentile.
    pub p99: u64,
    /// Largest observation.
    pub max: u64,
}

impl LatencyStats {
    /// Compute the stats from an unsorted slice. The input is sorted
    /// in-place; cost is `O(n log n)`. Returns `Self::default()` on
    /// empty input.
    pub fn from_samples(samples: &mut [u64]) -> Self {
        if samples.is_empty() {
            return Self::default();
        }
        samples.sort_unstable();
        let n = samples.len();
        let sum: u128 = samples.iter().map(|&x| x as u128).sum();
        let mean = sum as f64 / n as f64;
        Self {
            samples: n,
            mean,
            p50: percentile(samples, 0.50),
            p95: percentile(samples, 0.95),
            p99: percentile(samples, 0.99),
            max: *samples.last().unwrap(),
        }
    }
}

/// Nearest-rank percentile on a pre-sorted slice. `q ∈ [0.0, 1.0]`.
///
/// Uses `f64` directly so that exact decimal quantiles like `0.99`
/// don't drift through an `f32` round-trip and produce off-by-one
/// indices.
fn percentile(sorted: &[u64], q: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let n = sorted.len();
    // Nearest-rank: ceil(q * n) - 1, clamped.
    let idx_f = (q * n as f64).ceil() as isize - 1;
    let idx = idx_f.clamp(0, n as isize - 1) as usize;
    sorted[idx]
}

/// Aggregate metric set for a single trajectory.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PerTrajectory {
    /// Trajectory id within the suite.
    pub trajectory_id: u32,
    /// Number of `TraceStep`s executed.
    pub steps: usize,
    /// Belief revisions that actually triggered (AGM contraction).
    pub contradictions_detected: usize,
    /// BeliefShift / T-ECE for this trajectory. `None` when the
    /// runtime had no scored beliefs (defensive — should not happen
    /// in well-formed traces).
    pub tece: Option<f32>,
    /// Whether T-ECE met the configured `TECE_TARGET`.
    pub calibration_target_met: bool,
    /// Contract violation counters lifted from `SessionReport`.
    pub soft_violations: usize,
    /// See above.
    pub hard_violations: usize,
    /// See above.
    pub critical_violations: usize,
    /// Mean variational free energy across the trajectory, when the
    /// `active-inference` feature is enabled and a monitor was
    /// attached.
    pub free_energy_mean: Option<f64>,
    /// Number of `observe()` calls the FEP monitor performed (one per
    /// insert when wired).
    pub free_energy_samples: usize,
    /// Per-step insert latencies (microseconds). Aggregated into
    /// [`LatencyStats`] by the suite-level summary.
    #[serde(skip_serializing)]
    pub insert_latencies_us: Vec<u64>,
}

/// Aggregate metrics for an entire suite run.
///
/// Built by [`MetricSet::from_trajectories`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MetricSet {
    /// Number of trajectories aggregated.
    pub trajectories: usize,
    /// Sum of `steps` across trajectories.
    pub total_steps: usize,
    /// Sum of `contradictions_detected` across trajectories.
    pub total_contradictions: usize,
    /// Mean T-ECE across the trajectories that produced one.
    pub tece: Option<f32>,
    /// Number of trajectories for which `calibration_target_met` is
    /// true.
    pub calibration_met: usize,
    /// Sum of soft contract violations.
    pub soft_violations: usize,
    /// Sum of hard contract violations.
    pub hard_violations: usize,
    /// Sum of critical contract violations.
    pub critical_violations: usize,
    /// Suite-wide mean free energy. `None` when no trajectory reported
    /// one.
    pub free_energy_mean: Option<f64>,
    /// Total `observe()` samples behind `free_energy_mean`.
    pub free_energy_samples: usize,
    /// Insert latency distribution across all trajectories.
    pub insert_latency_us: LatencyStats,
}

impl MetricSet {
    /// Fold a slice of [`PerTrajectory`] into a suite-level summary.
    pub fn from_trajectories(per: &[PerTrajectory]) -> Self {
        let n = per.len();
        if n == 0 {
            return Self::default();
        }

        let total_steps: usize = per.iter().map(|t| t.steps).sum();
        let total_contradictions: usize = per.iter().map(|t| t.contradictions_detected).sum();

        let tece_samples: Vec<f32> = per.iter().filter_map(|t| t.tece).collect();
        let tece = if tece_samples.is_empty() {
            None
        } else {
            Some(tece_samples.iter().sum::<f32>() / tece_samples.len() as f32)
        };
        let calibration_met = per.iter().filter(|t| t.calibration_target_met).count();
        let soft_violations: usize = per.iter().map(|t| t.soft_violations).sum();
        let hard_violations: usize = per.iter().map(|t| t.hard_violations).sum();
        let critical_violations: usize = per.iter().map(|t| t.critical_violations).sum();

        // Free energy: weighted mean over trajectories that produced
        // any sample.
        let mut fe_sum = 0.0_f64;
        let mut fe_n = 0_usize;
        for t in per {
            if let Some(mean) = t.free_energy_mean {
                fe_sum += mean * t.free_energy_samples as f64;
                fe_n += t.free_energy_samples;
            }
        }
        let free_energy_mean = if fe_n == 0 { None } else { Some(fe_sum / fe_n as f64) };

        // Insert latencies: flatten into one Vec, then summarise.
        let mut all_latencies: Vec<u64> =
            per.iter().flat_map(|t| t.insert_latencies_us.iter().copied()).collect();
        let insert_latency_us = LatencyStats::from_samples(&mut all_latencies);

        Self {
            trajectories: n,
            total_steps,
            total_contradictions,
            tece,
            calibration_met,
            soft_violations,
            hard_violations,
            critical_violations,
            free_energy_mean,
            free_energy_samples: fe_n,
            insert_latency_us,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_samples_yield_default_stats() {
        let mut v: Vec<u64> = Vec::new();
        let s = LatencyStats::from_samples(&mut v);
        assert_eq!(s.samples, 0);
        assert_eq!(s.p50, 0);
        assert_eq!(s.p99, 0);
    }

    #[test]
    fn percentile_basic_cases() {
        let mut v: Vec<u64> = (1..=100).collect();
        let s = LatencyStats::from_samples(&mut v);
        assert_eq!(s.samples, 100);
        // Nearest-rank: p50 over 100 samples = element 50 (1-indexed)
        // = sorted[49] = 50.
        assert_eq!(s.p50, 50);
        assert_eq!(s.p95, 95);
        assert_eq!(s.p99, 99);
        assert_eq!(s.max, 100);
    }

    #[test]
    fn metric_set_aggregates_correctly() {
        let per = vec![
            PerTrajectory {
                trajectory_id: 0,
                steps: 5,
                contradictions_detected: 1,
                tece: Some(0.05),
                calibration_target_met: true,
                soft_violations: 0,
                hard_violations: 0,
                critical_violations: 0,
                free_energy_mean: Some(2.0),
                free_energy_samples: 5,
                insert_latencies_us: vec![10, 20, 30, 40, 50],
            },
            PerTrajectory {
                trajectory_id: 1,
                steps: 7,
                contradictions_detected: 2,
                tece: Some(0.07),
                calibration_target_met: false,
                soft_violations: 1,
                hard_violations: 0,
                critical_violations: 0,
                free_energy_mean: Some(3.0),
                free_energy_samples: 7,
                insert_latencies_us: vec![15, 25, 35, 45, 55, 65, 75],
            },
        ];
        let m = MetricSet::from_trajectories(&per);
        assert_eq!(m.trajectories, 2);
        assert_eq!(m.total_steps, 12);
        assert_eq!(m.total_contradictions, 3);
        // f32 mean drifts slightly off 0.06; compare with tolerance.
        let tece = m.tece.unwrap();
        assert!((tece - 0.06_f32).abs() < 1e-6, "got {tece}");
        assert_eq!(m.calibration_met, 1);
        assert_eq!(m.soft_violations, 1);
        // FE weighted mean: (2.0*5 + 3.0*7) / 12 = 31/12 ≈ 2.583
        let fe = m.free_energy_mean.unwrap();
        assert!((fe - 31.0 / 12.0).abs() < 1e-9);
        assert_eq!(m.free_energy_samples, 12);
        assert_eq!(m.insert_latency_us.samples, 12);
    }

    #[test]
    fn metric_set_with_no_fe_samples_is_none() {
        let per = vec![PerTrajectory {
            trajectory_id: 0,
            steps: 3,
            tece: Some(0.04),
            insert_latencies_us: vec![1, 2, 3],
            ..Default::default()
        }];
        let m = MetricSet::from_trajectories(&per);
        assert!(m.free_energy_mean.is_none());
        assert_eq!(m.free_energy_samples, 0);
    }
}
