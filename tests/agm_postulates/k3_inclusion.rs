//! K*3 (Inclusion): K*φ ⊆ Cn(K + {φ}).
//! The revised set is a subset of what's derivable from the original beliefs plus φ.

use epica_core::{BeliefNode, BeliefQuad, BeliefValue, Provenance};

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
