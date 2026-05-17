//! Basic BeliefQuad integration tests: insert → revise → diff → rollback.

use epica_core::{BeliefNode, BeliefQuad, BeliefValue, CausalEdge, Provenance};

fn make_llm_provenance() -> Provenance {
    Provenance::LlmInference {
        model: "claude-sonnet-4-6".into(),
        call_id: uuid::Uuid::new_v4(),
        prompt_hash: 42,
    }
}

#[test]
fn insert_and_get() {
    let mut quad = BeliefQuad::new();
    let id = quad.insert(BeliefNode::new(
        "user_intent",
        BeliefValue::Inferred(serde_json::json!("refactor auth")),
        make_llm_provenance(),
        0.85,
    ));

    let node = quad.get(id).unwrap();
    assert_eq!(node.key, "user_intent");
    assert!((node.fast_confidence - 0.85).abs() < 1e-6);
}

#[test]
fn revise_then_diff() {
    let mut quad = BeliefQuad::new();
    let id = quad.insert(BeliefNode::new(
        "x",
        BeliefValue::Asserted("original".into()),
        Provenance::UserStatement { turn: 0 },
        0.6,
    ));

    let empty = BeliefQuad::new();
    let diff_before = quad.diff(&empty);
    assert_eq!(diff_before.added.len(), 1);

    quad.revise(
        id,
        BeliefValue::Asserted("revised".into()),
        Provenance::UserStatement { turn: 1 },
        0.9,
    )
    .unwrap();

    let node = quad.get(id).unwrap();
    assert_eq!(node.value, BeliefValue::Asserted("revised".into()));
}

#[test]
fn checkpoint_and_rollback() {
    let mut quad = BeliefQuad::new();
    quad.insert(BeliefNode::new(
        "fact",
        BeliefValue::Deterministic(serde_json::json!(1)),
        Provenance::UserStatement { turn: 0 },
        1.0,
    ));

    let cp = quad.checkpoint();

    // Mutate: add a conflicting belief
    quad.insert(BeliefNode::new(
        "fact2",
        BeliefValue::Deterministic(serde_json::json!(2)),
        Provenance::UserStatement { turn: 1 },
        0.8,
    ));

    assert_eq!(quad.len(), 2);

    let diff = quad.rollback_to(cp).unwrap();
    assert!(diff.has_contradictions(), "rollback diff should indicate changes were reverted");
    assert_eq!(quad.len(), 1, "should be back to one belief after rollback");
}

#[test]
fn counterfactual_query_excludes_descendants() {
    let mut quad = BeliefQuad::new();

    let root = quad.insert(BeliefNode::new(
        "root",
        BeliefValue::Asserted("root cause".into()),
        Provenance::UserStatement { turn: 0 },
        0.9,
    ));

    let child = quad.insert(BeliefNode::new(
        "child",
        BeliefValue::Inferred(serde_json::json!("consequence")),
        make_llm_provenance(),
        0.7,
    ));

    quad.add_causal_edge(
        root,
        child,
        CausalEdge::InferredFrom { premises: vec![root] },
    );

    let cf = quad.counterfactual_query(root);
    assert!(cf.excluded.contains(&root));
    assert!(cf.excluded.contains(&child));
    assert_eq!(cf.surviving_count(), 0);
}
