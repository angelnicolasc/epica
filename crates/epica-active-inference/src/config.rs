//! Monitor configuration.
//!
//! `MonitorConfig` holds the three knobs that determine when free-energy
//! readings escalate to a [`SurpriseSignal`](crate::monitor::SurpriseSignal)
//! with `exceeds_budget = true`. Defaults are chosen to be quiet on a
//! healthy quad and to fire visibly when the agent drifts off its model.

use serde::{Deserialize, Serialize};

/// Tunable thresholds for [`ActiveInferenceMonitor`](crate::ActiveInferenceMonitor).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct MonitorConfig {
    /// Number of std-deviations above the rolling mean of `F` that count
    /// as a "surprise spike". Used as the `delta` in
    /// [`SurpriseSignal::is_spike`](crate::monitor::SurpriseSignal::is_spike).
    ///
    /// Default: `3.0` — empirically the band where Gaussian-looking noise
    /// fades and structural drift dominates.
    pub surprise_threshold: f64,

    /// Hard ceiling on a single observation's free energy. When `F >
    /// homeostatic_budget`, `SurpriseSignal::exceeds_budget` is `true` —
    /// the monitor's headline signal for "the agent is no longer in
    /// homeostasis."
    ///
    /// Default: `10.0` nats. For reference, a fully-confident belief
    /// (`c = 0.99`) against a flat prior (`π = 0.5`) contributes
    /// `KL ≈ 0.71`; a contradiction (`c = 0.99`, `π = 0.01`)
    /// contributes `KL ≈ 4.5`. A 10-nat budget therefore tolerates
    /// roughly 2–3 strong contradictions before tripping.
    pub homeostatic_budget: f64,

    /// Rolling history length used by `is_spike()` and `mean_free_energy()`.
    /// Clamped to `[16, 16_384]` to keep the rolling-stats cost cheap and
    /// the memory footprint predictable (~128 KB even at the upper bound).
    ///
    /// Default: `256`.
    pub history_capacity: usize,
}

impl Default for MonitorConfig {
    fn default() -> Self {
        Self {
            surprise_threshold: 3.0,
            homeostatic_budget: 10.0,
            history_capacity: 256,
        }
    }
}

impl MonitorConfig {
    /// Clamp `history_capacity` into the supported range. Called inside
    /// the monitor constructor so callers can't smuggle in a 0 (which
    /// would crash the rolling-stats math) or a 1B value (memory).
    pub(crate) fn sanitized_capacity(&self) -> usize {
        self.history_capacity.clamp(16, 16_384)
    }
}
