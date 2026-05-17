//! K*5 (Consistency): K*φ = K_false only if φ is inconsistent.
//! A consistent new belief produces a non-empty revised set.

use epica_core::{BeliefNode, BeliefQuad, BeliefValue, Provenance};

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
