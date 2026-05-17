//! K*4 (Vacuity): if ¬φ ∉ K, then K ⊆ K*φ.
//! If the current state does not contradict the new information, no beliefs are removed.

use epica_core::{BeliefNode, BeliefQuad, BeliefValue, Provenance};
use proptest::prelude::*;

#[test]
fn k4_same_value_is_expand_only() {
    let mut quad = BeliefQuad::new();
    let id = quad.insert(BeliefNode::new(
        "the_belief",
        BeliefValue::Asserted("hello".into()),
        Provenance::UserStatement { turn: 0 },
        0.8,
    ));

    // Revising with the EXACT same value cannot contradict the current state
    let result = quad
        .revise(
            id,
            BeliefValue::Asserted("hello".into()),
            Provenance::UserStatement { turn: 1 },
            0.9,
        )
        .unwrap();

    assert!(result.is_expansion(), "K*4: same-value revision must be expand-only (no contraction)");
    assert!(result.contracted.is_empty(), "K*4: no beliefs should be contracted");
}

proptest! {
    #[test]
    fn k4_vacuity_expand_only_proptest(
        key in "[a-z]{3,8}",
        value_str in "[a-z]{3,15}",
        confidence in 0.1f32..=1.0f32,
    ) {
        let mut quad = BeliefQuad::new();
        let id = quad.insert(BeliefNode::new(
            key,
            BeliefValue::Asserted(value_str.clone()),
            Provenance::UserStatement { turn: 0 },
            confidence,
        ));

        let size_before = quad.len();

        // Revising with the same value should never contract
        let result = quad.revise(
            id,
            BeliefValue::Asserted(value_str),
            Provenance::UserStatement { turn: 1 },
            confidence,
        ).unwrap();

        prop_assert!(result.contracted.is_empty(), "K*4: no contraction for non-contradicting revision");
        prop_assert_eq!(quad.len(), size_before, "K*4: no beliefs removed in vacuous revision");
    }
}
