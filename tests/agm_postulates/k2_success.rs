//! K*2 (Success): after revision, the revised set contains the new information.

use epica_core::{BeliefId, BeliefNode, BeliefQuad, BeliefValue, Provenance};
use proptest::prelude::*;

fn make_quad_with_node() -> (BeliefQuad, BeliefId) {
    let mut quad = BeliefQuad::new();
    let node = BeliefNode::new(
        "test_belief",
        BeliefValue::Inferred(serde_json::json!("original")),
        Provenance::UserStatement { turn: 0 },
        0.7,
    );
    let id = quad.insert(node);
    (quad, id)
}

#[test]
fn k2_success_after_revision() {
    let (mut quad, id) = make_quad_with_node();
    let new_value = BeliefValue::Inferred(serde_json::json!("revised"));

    let result = quad.revise(
        id,
        new_value.clone(),
        Provenance::UserStatement { turn: 1 },
        0.9,
    );

    assert!(result.is_ok(), "revision should succeed");
    let node = quad.get(id).expect("belief should still exist after revision");
    assert_eq!(node.value, new_value, "K*2: revised value must be present after revision");
}

proptest! {
    #[test]
    fn k2_success_proptest(
        original_str in "[a-z]{3,10}",
        new_str in "[a-z]{3,10}",
        confidence in 0.0f32..=1.0f32,
    ) {
        let mut quad = BeliefQuad::new();
        let node = BeliefNode::new(
            "prop_belief",
            BeliefValue::Asserted(original_str),
            Provenance::UserStatement { turn: 0 },
            confidence,
        );
        let id = quad.insert(node);

        let new_value = BeliefValue::Asserted(new_str.clone());
        let result = quad.revise(
            id,
            new_value.clone(),
            Provenance::UserStatement { turn: 1 },
            0.8,
        );

        prop_assert!(result.is_ok());
        let node = quad.get(id).expect("belief must exist after revision");
        prop_assert_eq!(&node.value, &new_value, "K*2: revised set must contain new information");
    }
}
