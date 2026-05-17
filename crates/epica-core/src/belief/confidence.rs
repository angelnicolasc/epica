/// Default divergence threshold τ for System 2 activation (`|fast − baseline| > τ`).
///
/// **Origin**: borrowed from AUQ (arXiv:2601.15703) §3.3, which reports τ ≈ 0.15 as
/// optimal on ALFWorld and WebShop benchmarks — measured over Transformer attention
/// weights, not over a belief graph.
///
/// **Limitation**: this runtime propagates confidence over a Noisy-OR belief graph,
/// not over attention weights. The two quantities are not directly comparable; τ = 0.15
/// has not been independently validated on Epica's runtime. See DEVLOG HD-E4 for the
/// planned calibration sweep. Override per-belief with `.with_reflection_threshold(τ)`,
/// or globally via `BeliefRuntime::with_default_tau(τ)`.
pub const DEFAULT_REFLECTION_THRESHOLD: f32 = 0.15;

/// Returns the effective confidence to use for downstream decisions.
///
/// If System 2 has reflected on this belief, its calibrated estimate takes precedence.
/// Otherwise, the fast System 1 estimate is used.
#[inline]
pub fn effective_confidence(fast: f32, slow: Option<f32>) -> f32 {
    slow.unwrap_or(fast)
}

/// Returns `true` if System 2 should be activated for this belief.
///
/// Activation triggers when `|fast_confidence - baseline| > threshold`.
/// The `baseline` is the agent's reliability threshold (not the same as fast_confidence itself).
#[inline]
pub fn should_activate_system2(fast: f32, baseline: f32, threshold: f32) -> bool {
    (fast - baseline).abs() > threshold
}

/// Combines multiple independent confidence values using the Noisy-OR formula.
///
/// `1.0 - ∏(1 - c_i)`
///
/// Assumes statistical independence between premises. For correlated premises,
/// use a more sophisticated combination method.
#[inline]
pub fn noisy_or(confidences: &[f32]) -> f32 {
    if confidences.is_empty() {
        return 0.0;
    }
    1.0 - confidences.iter().map(|&c| 1.0 - c.clamp(0.0, 1.0)).product::<f32>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noisy_or_zero_premises_is_zero() {
        assert_eq!(noisy_or(&[]), 0.0);
    }

    #[test]
    fn noisy_or_single_is_passthrough() {
        assert!((noisy_or(&[0.8]) - 0.8).abs() < 1e-6);
    }

    #[test]
    fn noisy_or_is_monotone() {
        let low = noisy_or(&[0.5, 0.5]);
        let high = noisy_or(&[0.9, 0.9]);
        assert!(high > low);
    }

    #[test]
    fn effective_confidence_prefers_slow() {
        assert!((effective_confidence(0.3, Some(0.8)) - 0.8).abs() < 1e-6);
    }
}
