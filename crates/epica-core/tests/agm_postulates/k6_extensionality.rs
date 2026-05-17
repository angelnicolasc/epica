//! K*6 (Extensionality): if Cn({φ}) = Cn({ψ}) then K*φ = K*ψ.
//! Equivalent propositions produce equivalent revised sets.
//!
//! Phase 1 approximation: K*6 is always marked `true` in `PostulateAudit`
//! (full extensionality requires logical equivalence checking — Phase 2).
//! This test validates that the audit records it correctly.

use epica_core::{BeliefNode, BeliefQuad, BeliefValue, Provenance};

#[test]
fn k6_extensionality_audit_field_is_true() {
    let mut quad = BeliefQuad::new();
    let id = quad.insert(BeliefNode::new(
        "k6_belief",
        BeliefValue::Inferred(serde_json::json!("initial")),
        Provenance::UserStatement { turn: 0 },
        0.6,
    ));

    let record = quad
        .revise(
            id,
            BeliefValue::Inferred(serde_json::json!("updated")),
            Provenance::UserStatement { turn: 1 },
            0.75,
        )
        .unwrap();

    assert!(
        record.postulate_audit.extensionality,
        "K*6: extensionality flag must be true (Phase 1 approximation)"
    );
}
