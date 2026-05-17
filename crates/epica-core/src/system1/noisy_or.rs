/// Combine independent confidence values via the Noisy-OR formula.
///
/// `1.0 - ∏(1.0 - c_i)`
///
/// Assumes independence between input confidences. For the causal graph, this means
/// each premise is an independent evidence source for the conclusion.
///
/// Returns `0.0` for an empty slice.
#[inline]
pub fn combine(confidences: &[f32]) -> f32 {
    if confidences.is_empty() {
        return 0.0;
    }
    1.0 - confidences
        .iter()
        .map(|&c| 1.0_f32 - c.clamp(0.0, 1.0))
        .product::<f32>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_is_zero() {
        assert_eq!(combine(&[]), 0.0);
    }

    #[test]
    fn single_passthrough() {
        let val = 0.7;
        assert!((combine(&[val]) - val).abs() < 1e-6);
    }

    #[test]
    fn two_perfect_confidences_give_one() {
        assert!((combine(&[1.0, 1.0]) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn two_half_confidences() {
        // 1 - (0.5)(0.5) = 0.75
        assert!((combine(&[0.5, 0.5]) - 0.75).abs() < 1e-6);
    }

    #[test]
    fn monotonically_increases_with_more_evidence() {
        let one = combine(&[0.6]);
        let two = combine(&[0.6, 0.6]);
        let three = combine(&[0.6, 0.6, 0.6]);
        assert!(one < two);
        assert!(two < three);
    }
}
