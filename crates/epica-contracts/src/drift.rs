//! Drift bounds for behavioral contracts.
//!
//! From Agent Behavioral Contracts (arXiv:2602.22302).

/// The expected drift bound D* = α/γ from Agent Behavioral Contracts (arXiv:2602.22302).
///
/// - `α`: natural drift rate of the agent (fraction of steps where the agent deviates).
/// - `γ`: enforcement rate of the contract (fraction of violations caught and corrected).
/// - `D* = α/γ`: expected number of undetected violations per enforcement window.
///
/// `gaussian_concentration` is the standard deviation of D* under a Gaussian CLT
/// approximation: D* acts as a Bernoulli parameter and its per-step std dev is
/// σ = √(D* · (1 − min(D*, 1))).  This gives the one-sigma uncertainty on the drift
/// estimate, useful for computing (p, δ, k)-satisfaction bounds.
#[derive(Debug, Clone)]
pub struct DriftBound {
    pub alpha: f64,
    pub gamma: f64,
    /// Expected violations per window = α/γ.
    pub expected_drift: f64,
    /// Per-step standard deviation of the drift estimate under a Gaussian approximation.
    pub gaussian_concentration: f64,
}

impl DriftBound {
    pub fn new(alpha: f64, gamma: f64) -> Self {
        let expected_drift = if gamma > 0.0 {
            alpha / gamma
        } else {
            f64::INFINITY
        };
        // Gaussian CLT approximation: σ = √(D* · (1 − clamp(D*, 0, 1)))
        let clamped = expected_drift.min(1.0).max(0.0);
        let gaussian_concentration = (clamped * (1.0 - clamped)).sqrt();
        Self {
            alpha,
            gamma,
            expected_drift,
            gaussian_concentration,
        }
    }

    /// Returns `true` if the contract is within tolerance for (p, δ, k)-satisfaction.
    ///
    /// A simple check: expected drift must be below `delta` per `k` steps.
    pub fn satisfies(&self, delta: u32, k: u32) -> bool {
        if k == 0 {
            return true;
        }
        self.expected_drift * k as f64 <= delta as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_does_not_panic() {
        let b = DriftBound::new(0.1, 0.5);
        assert!((b.expected_drift - 0.2).abs() < 1e-10);
        assert!(b.gaussian_concentration >= 0.0);
    }

    #[test]
    fn zero_gamma_gives_infinity() {
        let b = DriftBound::new(0.1, 0.0);
        assert!(b.expected_drift.is_infinite());
    }

    #[test]
    fn satisfies_tight_bound() {
        let b = DriftBound::new(0.1, 1.0); // D* = 0.1
        assert!(b.satisfies(1, 5)); // 0.1 * 5 = 0.5 ≤ 1
        assert!(!b.satisfies(0, 5)); // 0.5 > 0
    }
}
