//! K*3 (Inclusion): K*φ ⊆ Cn(K + {φ}).
//! The revised set is a subset of what's derivable from the original beliefs plus φ.

use epica_core::{BeliefNode, BeliefQuad, BeliefValue, Provenance};
use proptest::prelude::*;

#[test]
fn k3_inclusion_expand_only_preserves_other_beliefs() {
    let mut quad = BeliefQuad::new();

    // Insert two beliefs — one to revise, one to check is preserved
    let a = quad.insert(BeliefNode::new(
        "belief_a",
        BeliefValue::Deterministic(serde_json::json!(42)),
        Provenance::UserStatement { turn: 0 },
        1.0,
    ));
    let _b = quad.insert(BeliefNode::new(
        "belief_b",
        BeliefValue::Asserted("unrelated".into()),
        Provenance::UserStatement { turn: 0 },
        0.9,
    ));

    let count_before = quad.len();

    // Revise belief_a with the SAME value (expand-only, no contradiction)
    let result = quad.revise(
        a,
        BeliefValue::Deterministic(serde_json::json!(42)),
        Provenance::UserStatement { turn: 1 },
        0.95,
    );

    assert!(result.is_ok());
    let record = result.unwrap();
    assert!(record.is_expansion(), "K*4 vacuity: same-value revision should be expand-only");

    // K*3: belief_b must still be in the quad (no spurious contraction)
    assert_eq!(quad.len(), count_before, "K*3: non-contradicting beliefs must be preserved");
    assert!(
        quad.iter().any(|(_, n)| n.key == "belief_b"),
        "K*3: belief_b must survive a non-contradicting revision"
    );
}

proptest! {
    /// K*3 over arbitrary sibling beliefs: revising a target with the same value
    /// (vacuity / expand-only path) must never remove any unrelated belief.
    ///
    /// Generates 1..16 unrelated companion beliefs with random keys and values,
    /// inserts them alongside a target, then expands the target. The count and
    /// keyset must be preserved.
    #[test]
    fn k3_expand_only_preserves_all_siblings(
        sibling_count in 1usize..16,
        target_val in "[a-z]{3,12}",
        sibling_keys in proptest::collection::vec("[a-z]{3,12}", 1..16),
    ) {
        let mut quad = BeliefQuad::new();

        let target = quad.insert(BeliefNode::new(
            "target",
            BeliefValue::Asserted(target_val.clone()),
            Provenance::UserStatement { turn: 0 },
            0.7,
        ));

        // Insert sibling beliefs with unique synthetic keys to avoid collisions
        let n = sibling_count.min(sibling_keys.len());
        let mut sibling_keyset = std::collections::HashSet::new();
        for (i, key) in sibling_keys.iter().take(n).enumerate() {
            let unique_key = format!("sibling_{i}_{key}");
            quad.insert(BeliefNode::new(
                unique_key.clone(),
                BeliefValue::Asserted(format!("v_{i}")),
                Provenance::UserStatement { turn: 0 },
                0.8,
            ));
            sibling_keyset.insert(unique_key);
        }

        let count_before = quad.len();

        // Expand-only: revise target with the SAME value → vacuity holds → no contraction
        let result = quad.revise(
            target,
            BeliefValue::Asserted(target_val),
            Provenance::UserStatement { turn: 1 },
            0.95,
        );

        prop_assert!(result.is_ok());
        prop_assert!(result.unwrap().is_expansion(), "K*4 vacuity → expand-only");

        prop_assert_eq!(
            quad.len(),
            count_before,
            "K*3: expand-only must not remove any sibling belief"
        );
        for k in &sibling_keyset {
            prop_assert!(
                quad.iter().any(|(_, n)| &n.key == k),
                "K*3: sibling key {} must survive expand-only revision",
                k
            );
        }
    }
}
