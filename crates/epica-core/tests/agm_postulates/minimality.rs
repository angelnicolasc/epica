//! E-5: Minimal hitting-set contraction — verifies Hansson's Core-Retainment.
//!
//! When a belief B has two independent support paths and one path has multiple
//! joint premises, the contraction set must be a minimal hitting set: exactly
//! one premise removed per path, choosing the weakest (lowest confidence) from
//! multi-premise paths. The old `flatten().collect()` implementation removed ALL
//! premises from ALL paths — violating minimality when any path has > 1 premise.
//!
//! Scenario:
//!   Path 1: A ∧ C → B  (joint premises; conf(A) = 0.3 < conf(C) = 0.7)
//!   Path 2: D → B       (independent single-premise support)
//!
//! Correct contraction: {A, D}  (size 2)
//! Old flat-collect bug: {A, C, D}  (size 3, removes C unnecessarily)

use epica_core::{BeliefNode, BeliefQuad, BeliefValue, CausalEdge, Provenance};

#[test]
fn contraction_is_minimal_with_independent_paths() {
    let mut quad = BeliefQuad::new();

    // A: low confidence (0.3) — the weakest premise in path 1; must be contracted
    let a_id = quad.insert(BeliefNode::new(
        "a",
        BeliefValue::Asserted("a_value".into()),
        Provenance::UserStatement { turn: 0 },
        0.3,
    ));

    // C: higher confidence (0.7) — stronger premise in path 1; must NOT be contracted
    let c_id = quad.insert(BeliefNode::new(
        "c",
        BeliefValue::Asserted("c_value".into()),
        Provenance::UserStatement { turn: 0 },
        0.7,
    ));

    // D: sole premise in path 2; must be contracted
    let d_id = quad.insert(BeliefNode::new(
        "d",
        BeliefValue::Asserted("d_value".into()),
        Provenance::UserStatement { turn: 0 },
        0.8,
    ));

    // B: the belief being revised (high confidence so it stays after expansion)
    let b_id = quad.insert(BeliefNode::new(
        "b",
        BeliefValue::Asserted("original".into()),
        Provenance::UserStatement { turn: 0 },
        0.9,
    ));

    // Path 1: A ∧ C jointly support B — one InferredFrom edge with two premises.
    // Minimal hitting set picks the weakest: A (0.3), leaving C intact.
    quad.add_causal_edge(a_id, b_id, CausalEdge::InferredFrom { premises: vec![a_id, c_id] });

    // Path 2: D independently supports B — single-premise path, D must be contracted.
    quad.add_causal_edge(d_id, b_id, CausalEdge::InferredFrom { premises: vec![d_id] });

    // Revise B with a contradicting value — triggers contraction before expansion.
    let record = quad
        .revise(
            b_id,
            BeliefValue::Asserted("revised".into()),
            Provenance::UserStatement { turn: 1 },
            0.9,
        )
        .expect("revision should succeed");

    // Minimal hitting set: A (weakest in path 1) + D (sole premise in path 2).
    // Old flat-collect would have produced {A, C, D} (size 3).
    assert_eq!(
        record.contracted.len(),
        2,
        "contraction must be minimal: 2 beliefs removed, not 3. contracted = {:?}",
        record.contracted,
    );
    assert!(
        record.contracted.contains(&a_id),
        "A (weakest premise in path 1) must be contracted"
    );
    assert!(
        record.contracted.contains(&d_id),
        "D (sole premise in path 2) must be contracted"
    );
    assert!(
        !record.contracted.contains(&c_id),
        "C must NOT be contracted — it is the stronger premise in path 1 and Core-Retainment forbids unnecessary removal"
    );

    // The revised belief B must still exist with the new value (K*2 — success).
    let b_after = quad.get(b_id).expect("B must exist after revision");
    assert_eq!(
        b_after.value,
        BeliefValue::Asserted("revised".into()),
        "K*2: B must hold the new value after revision"
    );
}
