//! Variational free energy over the `BeliefQuad`.
//!
//! The math here is intentionally elementary — Bernoulli posteriors, KL
//! divergences in closed form, Noisy-OR for the structural prior. The
//! point is reproducibility, not novelty.
//!
//! ### Formulae
//!
//! For a belief `i` with `fast_confidence c_i`:
//!
//! - Posterior `q_i(s_i = true)  = c_i`,
//!             `q_i(s_i = false) = 1 - c_i`.
//! - Prior derived from the causal graph:
//!     - `π_i = 0.5` when belief `i` has no causal predecessor (the
//!       uninformative / "no commitment" prior),
//!     - `π_i = NoisyOr(parent confidences)` otherwise. NoisyOr is what
//!       `epica-core::system1::noisy_or` already uses, so the active-
//!       inference prior agrees with System 1's propagation by
//!       construction.
//! - Per-belief KL contribution (Bernoulli-to-Bernoulli, closed form):
//!     `KL(q_i || p_i) = c_i · log(c_i / π_i) + (1 - c_i) · log((1 - c_i) /
//!                                                              (1 - π_i))`
//!   with the standard `0 · log 0 = 0` convention applied.
//! - Expected log-likelihood of the most recent observation, treating
//!   `c_obs` as both the agent's belief in `o = true` and the inverse of
//!   the measurement noise:
//!     `E_q[log p(o | s)]  = c · log c_obs  +  (1 - c) · log(1 - c_obs)`.
//! - Total variational free energy:
//!     `F  =  Σ_i KL(q_i || p_i)  -  E_q[log p(o_last | s_last)]`.

use epica_core::{BeliefId, BeliefNode, BeliefQuad, BeliefValue};

/// Numerical guard: any probability is clamped into `[EPS, 1 - EPS]` before
/// being passed to `ln`, so that singularities at `0` or `1` don't poison
/// the free-energy sum. `1e-9` is small enough not to bias decisions yet
/// large enough to keep `ln` finite in `f64`.
const EPS: f64 = 1e-9;

/// Closed-form KL divergence between two Bernoulli(p) distributions.
///
/// `KL(p || q) = p · ln(p/q) + (1-p) · ln((1-p)/(1-q))`.
///
/// Both inputs are clamped into `[EPS, 1 - EPS]` to keep the result
/// finite. Returns a non-negative `f64`.
#[must_use]
pub fn bernoulli_kl(p: f64, q: f64) -> f64 {
    let p = p.clamp(EPS, 1.0 - EPS);
    let q = q.clamp(EPS, 1.0 - EPS);
    p * (p / q).ln() + (1.0 - p) * ((1.0 - p) / (1.0 - q)).ln()
}

/// `E_q[ln p(o | s)]` for a Bernoulli observation channel.
///
/// `c_belief` is the agent's posterior on `s = true`; `c_observation` is the
/// reported confidence on `o = true` interpreted as the channel reliability.
/// Both inputs are clamped into `[EPS, 1 - EPS]`.
#[must_use]
pub fn expected_log_likelihood(c_belief: f64, c_observation: f64) -> f64 {
    let c = c_belief.clamp(EPS, 1.0 - EPS);
    let o = c_observation.clamp(EPS, 1.0 - EPS);
    c * o.ln() + (1.0 - c) * (1.0 - o).ln()
}

/// Compute the Noisy-OR aggregation of a set of confidences.
///
/// `NoisyOr(c_1, …, c_n) = 1 - Π (1 - c_i)`. Empty input → `0.5` (the
/// uninformative prior), which is what we want when a belief has no
/// causal predecessor.
fn noisy_or(confidences: impl IntoIterator<Item = f64>) -> f64 {
    let mut any = false;
    let mut not_any = 1.0;
    for c in confidences {
        let c = c.clamp(0.0, 1.0);
        not_any *= 1.0 - c;
        any = true;
    }
    if any {
        1.0 - not_any
    } else {
        0.5
    }
}

/// Compute the structural prior `π_i` for belief `id` from the causal
/// graph.
///
/// - When `id` has no `InferredFrom` parent path, returns `0.5` — the
///   "no commitment" Bernoulli prior.
/// - Otherwise, aggregates parent confidences via Noisy-OR over the flat
///   premise set. This is what `epica-core::system1` uses for forward
///   propagation, so the active-inference prior and the System 1 belief
///   coincide whenever the agent is in homeostasis — KL contributions
///   small.
pub fn structural_prior(quad: &BeliefQuad, id: BeliefId) -> f64 {
    let paths = quad.causal().inferred_from_premises(id);
    if paths.is_empty() {
        return 0.5;
    }
    // Flatten across paths; a parent that appears in two paths only
    // contributes once.
    let mut seen: Vec<BeliefId> = Vec::new();
    let mut confidences: Vec<f64> = Vec::new();
    for path in paths {
        for parent in path {
            if !seen.contains(&parent) {
                seen.push(parent);
                if let Some(node) = quad.get(parent) {
                    confidences.push(node.fast_confidence as f64);
                }
            }
        }
    }
    noisy_or(confidences)
}

/// Decomposition of the variational free energy computed over a `BeliefQuad`.
///
/// `f_total = kl_total - log_likelihood_obs`.
///
/// Each field is reported so callers can attribute spikes:
///
/// - `kl_total` rising: the agent's posterior has drifted away from the
///   causal-graph prior (belief revisions diverged from their predecessors).
/// - `log_likelihood_obs` falling (more negative): the latest observation
///   surprises the model.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FreeEnergyBreakdown {
    /// `Σ_i KL(q_i || p_i)`. Always non-negative.
    pub kl_total: f64,
    /// `E_q[ln p(o_last | s_last)]`. Always non-positive.
    pub log_likelihood_obs: f64,
    /// `F = kl_total - log_likelihood_obs`. Non-negative by Gibbs' inequality.
    pub f_total: f64,
    /// Number of beliefs summed into `kl_total`.
    pub n_beliefs: usize,
}

/// Compute the per-observation variational free energy.
///
/// Returns the decomposition; callers can either threshold on `f_total`
/// (the headline number) or drill into the parts.
#[must_use]
pub fn quad_free_energy(quad: &BeliefQuad, last_obs: &BeliefNode) -> FreeEnergyBreakdown {
    let mut kl_total = 0.0;
    let mut n_beliefs = 0usize;

    for (id, node) in quad.iter() {
        let c = node.fast_confidence as f64;
        let prior = structural_prior(quad, id);
        kl_total += bernoulli_kl(c, prior);
        n_beliefs += 1;
    }

    // Last-observation likelihood: use the observed node's posterior
    // against its own reported confidence — the reading "the channel
    // reports c_obs, the agent believed c_obs" yields the maximum-
    // likelihood term given the available signal.
    let c_obs = last_obs.fast_confidence as f64;
    let c_belief = match &last_obs.value {
        BeliefValue::Inferred(_) | BeliefValue::Deterministic(_) => c_obs,
        BeliefValue::Asserted(_) | BeliefValue::Reference(_) => c_obs,
    };
    let log_likelihood_obs = expected_log_likelihood(c_belief, c_obs);
    let f_total = kl_total - log_likelihood_obs;

    FreeEnergyBreakdown { kl_total, log_likelihood_obs, f_total, n_beliefs }
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
    fn kl_is_zero_when_distributions_match() {
        // KL(p || p) = 0
        for p in [0.05_f64, 0.3, 0.5, 0.7, 0.95] {
            assert!(bernoulli_kl(p, p).abs() < 1e-10,
                "KL(p || p) should be 0 for p = {p}");
        }
    }

    #[test]
    fn kl_is_non_negative() {
        for &(p, q) in &[(0.1, 0.9), (0.9, 0.1), (0.5, 0.01), (0.3, 0.7)] {
            assert!(bernoulli_kl(p, q) >= 0.0,
                "KL must be non-negative; got KL({p} || {q}) = {}",
                bernoulli_kl(p, q));
        }
    }

    #[test]
    fn kl_clamps_extreme_inputs() {
        // 0 and 1 must not produce NaN.
        let v = bernoulli_kl(0.0, 0.5);
        assert!(v.is_finite());
        let v = bernoulli_kl(1.0, 0.5);
        assert!(v.is_finite());
    }

    #[test]
    fn noisy_or_empty_is_uninformative() {
        assert_eq!(noisy_or(std::iter::empty()), 0.5);
    }

    #[test]
    fn noisy_or_combines_independent_supports() {
        // Two independent supports of 0.5 each: 1 - 0.5 * 0.5 = 0.75.
        let v = noisy_or([0.5, 0.5]);
        assert!((v - 0.75).abs() < 1e-9);
    }

    #[test]
    fn structural_prior_is_flat_without_parents() {
        let mut quad = BeliefQuad::new();
        let id = quad.insert(node("k", 0.7));
        assert!((structural_prior(&quad, id) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn free_energy_is_finite_and_non_negative() {
        let mut quad = BeliefQuad::new();
        let _ = quad.insert(node("a", 0.6));
        let _ = quad.insert(node("b", 0.4));
        let id = quad.insert(node("c", 0.9));
        let obs = quad.get(id).unwrap().clone();

        let f = quad_free_energy(&quad, &obs);
        assert!(f.f_total.is_finite());
        assert!(f.kl_total >= 0.0,
            "kl_total must be non-negative, got {}", f.kl_total);
        assert!(f.log_likelihood_obs <= 0.0,
            "log_likelihood_obs must be non-positive, got {}", f.log_likelihood_obs);
        assert_eq!(f.n_beliefs, 3);
    }

    #[test]
    fn perfectly_calibrated_quad_has_low_kl() {
        // Each belief at 0.5 against the flat prior of 0.5 ⇒ KL = 0.
        let mut quad = BeliefQuad::new();
        let id = quad.insert(node("a", 0.5));
        let _ = quad.insert(node("b", 0.5));
        let obs = quad.get(id).unwrap().clone();

        let f = quad_free_energy(&quad, &obs);
        assert!(f.kl_total.abs() < 1e-9,
            "expected kl_total ≈ 0, got {}", f.kl_total);
    }

    #[test]
    fn overconfident_belief_dominates_kl() {
        let mut quad = BeliefQuad::new();
        let id = quad.insert(node("loud", 0.99));
        let obs = quad.get(id).unwrap().clone();

        let f = quad_free_energy(&quad, &obs);
        // A single belief at c=0.99 against π=0.5 contributes
        // KL(0.99 || 0.5) = 0.99·ln(0.99/0.5) + 0.01·ln(0.01/0.5)
        //                 = 0.99·0.6831 + 0.01·(-3.912)
        //                 ≈ 0.637 nats.
        assert!(f.kl_total > 0.55,
            "expected kl_total > 0.55 (Bernoulli KL of 0.99 vs 0.5); got {}",
            f.kl_total);
        assert!(f.kl_total < 0.75);
    }
}
