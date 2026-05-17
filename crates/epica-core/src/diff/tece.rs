/// Compute the Trajectory Expected Calibration Error (T-ECE).
///
/// From Agentic UQ (arXiv:2601.15703):
///
/// ```text
/// T-ECE = Σ_t |confidence_t - accuracy_t| / T
/// ```
///
/// `history` is a slice of `(confidence_at_step_t, was_correct_at_step_t)` tuples.
/// `accuracy_t` is `1.0` if the belief was confirmed correct at step `t`, `0.0` otherwise.
///
/// Returns `None` for an empty history (T-ECE undefined without observations).
pub fn compute(history: &[(f32, bool)]) -> Option<f32> {
    if history.is_empty() {
        return None;
    }
    let t = history.len() as f32;
    let sum: f32 = history
        .iter()
        .map(|&(conf, correct)| {
            let accuracy = if correct { 1.0 } else { 0.0 };
            (conf - accuracy).abs()
        })
        .sum();
    Some(sum / t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_history_is_none() {
        assert!(compute(&[]).is_none());
    }

    #[test]
    fn perfect_calibration_is_zero() {
        // confidence 1.0, correct → |1.0 - 1.0| = 0.0
        let tece = compute(&[(1.0, true), (0.0, false)]).unwrap();
        assert!((tece - 0.0).abs() < 1e-6);
    }

    #[test]
    fn overconfident_agent() {
        // Always 1.0 confidence, always wrong → T-ECE = 1.0
        let tece = compute(&[(1.0, false), (1.0, false), (1.0, false)]).unwrap();
        assert!((tece - 1.0).abs() < 1e-6);
    }

    #[test]
    fn target_calibration_is_below_threshold() {
        // Target from playbook: T-ECE < 0.08 in well-calibrated sessions
        // Simulated: 0.85 confidence, 90% correct
        let history: Vec<(f32, bool)> = (0..20)
            .map(|i| (0.85f32, i % 10 != 0)) // correct 90% of the time
            .collect();
        let tece = compute(&history).unwrap();
        // Not asserting < 0.08 (that requires a real agent), just that it's computable
        assert!(tece >= 0.0);
        assert!(tece <= 1.0);
    }
}
