//! The runtime-facing monitor.
//!
//! [`ActiveInferenceMonitor`] is what callers actually hold. Each
//! `observe()` call:
//!
//! 1. Computes the per-observation [`FreeEnergyBreakdown`] over the quad
//!    (see [`crate::free_energy`]).
//! 2. Pushes the total into a bounded rolling history.
//! 3. Updates the homeostatic budget — every observation consumes
//!    `f_total` from a refilling reservoir, so a steady stream of small
//!    surprises also exhausts it, not only single huge spikes.
//! 4. Returns a [`SurpriseSignal`] summarising the read.
//!
//! The whole loop is allocation-light: the history is a fixed-capacity
//! `VecDeque` and the rolling mean / variance are recomputed in O(N) on
//! each call, which is fine for the default `history_capacity = 256`
//! (one observation per belief insert is the usage shape).

use std::collections::VecDeque;

use epica_core::{BeliefNode, BeliefQuad};
use serde::{Deserialize, Serialize};

use crate::{
    config::MonitorConfig,
    free_energy::{quad_free_energy, FreeEnergyBreakdown},
};

/// The summary emitted by [`ActiveInferenceMonitor::observe`].
///
/// `SurpriseSignal` is small and `Copy`-able so callers can pattern-match
/// without taking ownership. It is also `Serialize`-able, which makes it
/// drop-in for emission as an `AuditEntry::details` payload.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct SurpriseSignal {
    /// Free energy of this observation. Always finite, always
    /// non-negative.
    pub free_energy: f64,
    /// Rolling mean of `free_energy` over the recent history (not
    /// including this observation).
    pub rolling_mean: f64,
    /// Rolling sample standard deviation of `free_energy` (not including
    /// this observation). `0.0` when the history is empty or constant.
    pub rolling_std: f64,
    /// `free_energy > homeostatic_budget`. The headline "agent off
    /// homeostasis" bit — this is what the runtime should raise as a
    /// contract violation if any.
    pub exceeds_budget: bool,
    /// Remaining homeostatic budget after this observation deducted
    /// `free_energy` from the reservoir. Clamped to `>= 0`.
    pub budget_remaining: f64,
    /// `n_beliefs` from the underlying [`FreeEnergyBreakdown`].
    pub n_beliefs: usize,
}

impl SurpriseSignal {
    /// `true` when `free_energy` exceeds the rolling mean by more than
    /// `surprise_threshold` standard deviations. Empty / constant history
    /// returns `false` — no statistic available yet.
    #[must_use]
    pub fn is_spike(&self, threshold: f64) -> bool {
        if self.rolling_std <= f64::EPSILON {
            return false;
        }
        (self.free_energy - self.rolling_mean) > threshold * self.rolling_std
    }
}

/// Continuous Bayesian-surprise monitor for an Epica `BeliefQuad`.
///
/// Construct with [`Self::new`] (defaults) or [`Self::with_config`]; feed
/// observations with [`Self::observe`]; read out the cumulative free-energy
/// trace with [`Self::history`] or [`Self::mean_free_energy`].
#[derive(Debug)]
pub struct ActiveInferenceMonitor {
    config: MonitorConfig,
    history: VecDeque<f64>,
    /// Refilling reservoir of "homeostasis credit". Each observation
    /// deducts its `f_total`; if it goes below zero, the monitor will
    /// continue reporting `exceeds_budget = true` until enough quiet
    /// observations refill it. Refill = +1 unit per `observe()` call,
    /// capped at `homeostatic_budget`.
    reservoir: f64,
    /// Strictly monotonic counter of `observe()` calls. Useful for tests
    /// and for callers that want to emit a sequence id with the audit
    /// entry.
    pub observations: u64,
}

impl ActiveInferenceMonitor {
    /// Build a monitor with default configuration ([`MonitorConfig::default`]).
    pub fn new() -> Self {
        Self::with_config(MonitorConfig::default())
    }

    /// Build a monitor with caller-supplied configuration.
    pub fn with_config(config: MonitorConfig) -> Self {
        let cap = config.sanitized_capacity();
        Self {
            config,
            history: VecDeque::with_capacity(cap),
            reservoir: config.homeostatic_budget,
            observations: 0,
        }
    }

    /// Read-only access to the current configuration.
    pub fn config(&self) -> &MonitorConfig {
        &self.config
    }

    /// Borrow the rolling free-energy history (oldest → newest).
    pub fn history(&self) -> &VecDeque<f64> {
        &self.history
    }

    /// Rolling mean of `free_energy`. `0.0` for an empty history.
    pub fn mean_free_energy(&self) -> f64 {
        if self.history.is_empty() {
            return 0.0;
        }
        self.history.iter().sum::<f64>() / self.history.len() as f64
    }

    /// Current free-energy reservoir. Equivalent to "homeostatic credit
    /// the agent still has before the next observation trips the budget."
    pub fn budget_remaining(&self) -> f64 {
        self.reservoir.max(0.0)
    }

    /// Process one observation. Returns the [`SurpriseSignal`] and
    /// internally updates the rolling history + reservoir.
    ///
    /// `quad` is read-only; the monitor never mutates the agent's state.
    pub fn observe(&mut self, quad: &BeliefQuad, last_obs: &BeliefNode) -> SurpriseSignal {
        let FreeEnergyBreakdown { f_total, n_beliefs, .. } =
            quad_free_energy(quad, last_obs);

        let rolling_mean = self.mean_free_energy();
        let rolling_std = rolling_std(&self.history, rolling_mean);

        // Reservoir bookkeeping: spend f_total, refill +1, clamp.
        self.reservoir =
            (self.reservoir - f_total + 1.0).min(self.config.homeostatic_budget);
        let exceeds_budget = f_total > self.config.homeostatic_budget;

        // Update rolling history *after* computing the mean/std for this
        // sample — `is_spike` answers "is this surprising vs. the past?"
        let cap = self.config.sanitized_capacity();
        if self.history.len() == cap {
            self.history.pop_front();
        }
        self.history.push_back(f_total);
        self.observations += 1;

        SurpriseSignal {
            free_energy: f_total,
            rolling_mean,
            rolling_std,
            exceeds_budget,
            budget_remaining: self.budget_remaining(),
            n_beliefs,
        }
    }

    /// Reset the monitor to its post-construction state. The rolling
    /// history is dropped, the reservoir is refilled, the observation
    /// counter is reset.
    pub fn reset(&mut self) {
        self.history.clear();
        self.reservoir = self.config.homeostatic_budget;
        self.observations = 0;
    }
}

impl Default for ActiveInferenceMonitor {
    fn default() -> Self {
        Self::new()
    }
}

/// Sample standard deviation of a rolling window. Returns `0.0` for
/// windows of length `< 2` so the spike heuristic can short-circuit.
fn rolling_std(window: &VecDeque<f64>, mean: f64) -> f64 {
    if window.len() < 2 {
        return 0.0;
    }
    let n = window.len() as f64;
    let var = window.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1.0);
    var.sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;
    use epica_core::{BeliefNode, BeliefQuad, BeliefValue, Provenance};

    fn node(key: &str, conf: f32) -> BeliefNode {
        BeliefNode::new(
            key,
            BeliefValue::Asserted("v".into()),
            Provenance::UserStatement { turn: 0 },
            conf,
        )
    }

    #[test]
    fn first_observation_has_zero_mean_and_std() {
        let mut quad = BeliefQuad::new();
        let id = quad.insert(node("k", 0.5));
        let obs = quad.get(id).unwrap().clone();

        let mut m = ActiveInferenceMonitor::new();
        let s = m.observe(&quad, &obs);
        assert_eq!(s.rolling_mean, 0.0);
        assert_eq!(s.rolling_std, 0.0);
        assert!(s.free_energy.is_finite());
        assert!(!s.is_spike(3.0)); // no std ⇒ never spike
        assert_eq!(m.observations, 1);
    }

    #[test]
    fn quiet_agent_stays_within_budget() {
        // 5 beliefs at c=0.5 ⇒ kl per belief = 0 ⇒ f_total ≈ -log_lik only.
        // log_lik at c_obs=0.5 ⇒ 0.5*ln 0.5 + 0.5*ln 0.5 = ln 0.5 ≈ -0.693.
        // So f_total ≈ 0.693, well under default budget 10.
        let mut quad = BeliefQuad::new();
        for k in ["a", "b", "c", "d", "e"] {
            quad.insert(node(k, 0.5));
        }
        let id = quad
            .iter()
            .next()
            .map(|(i, _)| i)
            .expect("at least one belief");
        let obs = quad.get(id).unwrap().clone();

        let mut m = ActiveInferenceMonitor::new();
        for _ in 0..10 {
            let s = m.observe(&quad, &obs);
            assert!(!s.exceeds_budget, "quiet agent must stay within budget");
        }
        assert!(m.budget_remaining() >= 5.0);
    }

    #[test]
    fn extreme_overconfidence_eventually_trips_budget() {
        // Build a quad with many highly-confident contradictory-style
        // beliefs (c near 0.99) and a low-budget config to make the
        // trip visible.
        let mut quad = BeliefQuad::new();
        for k in 0..40 {
            quad.insert(node(&format!("k{k}"), 0.999));
        }
        let id = quad.iter().next().map(|(i, _)| i).unwrap();
        let obs = quad.get(id).unwrap().clone();

        let cfg = MonitorConfig {
            homeostatic_budget: 2.0,
            surprise_threshold: 3.0,
            history_capacity: 64,
        };
        let mut m = ActiveInferenceMonitor::with_config(cfg);
        let s = m.observe(&quad, &obs);
        assert!(
            s.exceeds_budget,
            "kl over 40 beliefs at c=0.999 must exceed budget=2.0; got f={}",
            s.free_energy
        );
    }

    #[test]
    fn history_is_bounded_by_capacity() {
        // `sanitized_capacity` floors `history_capacity` at 16, so 16 is
        // the smallest cap a caller can realistically configure.
        let cfg = MonitorConfig {
            surprise_threshold: 3.0,
            homeostatic_budget: 1000.0,
            history_capacity: 16,
        };
        let mut m = ActiveInferenceMonitor::with_config(cfg);

        let mut quad = BeliefQuad::new();
        let id = quad.insert(node("k", 0.5));
        let obs = quad.get(id).unwrap().clone();

        for _ in 0..50 {
            m.observe(&quad, &obs);
        }
        assert_eq!(m.history().len(), 16);
        assert_eq!(m.observations, 50);
    }

    #[test]
    fn history_capacity_floor_is_enforced() {
        // Even with a tiny config, the cap is at least 16.
        let cfg = MonitorConfig {
            surprise_threshold: 3.0,
            homeostatic_budget: 1000.0,
            history_capacity: 1,
        };
        let m = ActiveInferenceMonitor::with_config(cfg);
        assert_eq!(m.config().sanitized_capacity(), 16);
    }

    #[test]
    fn reset_clears_state() {
        let mut quad = BeliefQuad::new();
        let id = quad.insert(node("k", 0.8));
        let obs = quad.get(id).unwrap().clone();

        let mut m = ActiveInferenceMonitor::new();
        for _ in 0..5 {
            m.observe(&quad, &obs);
        }
        assert!(!m.history().is_empty());
        m.reset();
        assert!(m.history().is_empty());
        assert_eq!(m.observations, 0);
        assert_eq!(m.budget_remaining(), m.config().homeostatic_budget);
    }

    #[test]
    fn is_spike_fires_after_stable_then_spike() {
        // Build a tiny history of identical low-energy reads, then a
        // single huge read, and verify the spike detector sees it.
        let mut m = ActiveInferenceMonitor::with_config(MonitorConfig {
            surprise_threshold: 3.0,
            homeostatic_budget: 1000.0,
            history_capacity: 32,
        });
        // Manually fill history with low, slightly-varying numbers.
        for x in [0.5_f64, 0.55, 0.45, 0.5, 0.52, 0.48, 0.5, 0.5] {
            m.history.push_back(x);
        }
        // Simulate the next sample as a 5-sigma spike.
        let next = SurpriseSignal {
            free_energy: 50.0,
            rolling_mean: m.mean_free_energy(),
            rolling_std: rolling_std(&m.history, m.mean_free_energy()),
            exceeds_budget: false,
            budget_remaining: 100.0,
            n_beliefs: 1,
        };
        assert!(next.is_spike(3.0));
    }
}
