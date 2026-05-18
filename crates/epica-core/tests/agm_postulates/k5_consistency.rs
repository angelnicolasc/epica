//! K*5 (Consistency): K*φ = K_false only if φ is inconsistent.
//! A consistent new belief produces a non-empty revised set.

use epica_core::{BeliefNode, BeliefQuad, BeliefValue, Provenance};
use proptest::prelude::*;

#[test]
fn k5_revision_with_consistent_value_produces_nonempty_quad() {
    let mut quad = BeliefQuad::new();
    let id = quad.insert(BeliefNode::new(
        "fact",
        BeliefValue::Deterministic(serde_json::json!(true)),
        Provenance::ToolResult {
            tool: "checker".into(),
            call_id: uuid::Uuid::new_v4(),
        },
        1.0,
    ));

    // Revise with a different consistent value
    let result = quad.revise(
        id,
        BeliefValue::Deterministic(serde_json::json!(false)),
        Provenance::ToolResult {
            tool: "checker".into(),
            call_id: uuid::Uuid::new_v4(),
        },
        0.95,
    );

    assert!(result.is_ok(), "K*5: consistent revision must not fail");
    assert!(!quad.is_empty(), "K*5: revised quad must be non-empty for consistent φ");

    let record = result.unwrap();
    assert!(
        record.postulate_audit.consistency,
        "K*5: consistency postulate must pass in audit"
    );
}

#[test]
fn k5_audit_consistency_flag_is_set() {
    let mut quad = BeliefQuad::new();
    let id = quad.insert(BeliefNode::new(
        "x",
        BeliefValue::Asserted("a".into()),
        Provenance::UserStatement { turn: 0 },
        0.7,
    ));

    let record = quad
        .revise(
            id,
            BeliefValue::Asserted("b".into()),
            Provenance::UserStatement { turn: 1 },
            0.8,
        )
        .unwrap();

    assert!(record.postulate_audit.consistency);
    assert!(record.postulate_audit.success);
}

proptest! {
    /// K*5 (Consistency) over arbitrary `Asserted` revisions: for ANY consistent
    /// new value (i.e. any non-empty string), the post-revision quad must remain
    /// non-empty and the consistency audit flag must be `true`.
    ///
    /// Generates random ASCII strings with length 1..32, ensuring `new_str` is
    /// always parseable as a consistent atom. Confidence is sampled across the
    /// full unit interval to exercise clamp paths.
    #[test]
    fn k5_arbitrary_consistent_revision(
        original in "[a-zA-Z0-9 ]{1,32}",
        new in "[a-zA-Z0-9 ]{1,32}",
        confidence in 0.0f32..=1.0f32,
    ) {
        let mut quad = BeliefQuad::new();
        let id = quad.insert(BeliefNode::new(
            "k5_prop",
            BeliefValue::Asserted(original),
            Provenance::UserStatement { turn: 0 },
            0.5,
        ));

        let result = quad.revise(
            id,
            BeliefValue::Asserted(new),
            Provenance::UserStatement { turn: 1 },
            confidence,
        );

        prop_assert!(result.is_ok(), "K*5: every consistent atom must be admissible");
        let record = result.unwrap();
        prop_assert!(record.postulate_audit.consistency, "K*5: consistency audit must hold");
        prop_assert!(!quad.is_empty(), "K*5: quad must remain non-empty");

        // Confidence clamp invariant — separate sanity check, but cheap to verify here.
        let node = quad.get(id).expect("revised node must exist");
        prop_assert!(
            (0.0..=1.0).contains(&node.fast_confidence),
            "fast_confidence must remain in [0, 1] after clamp"
        );
    }
}
