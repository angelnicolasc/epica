//! System 1 propagation correctness tests.

use epica_core::{BeliefNode, BeliefQuad, BeliefValue, CausalEdge, Provenance};

fn insert(quad: &mut BeliefQuad, key: &str, confidence: f32) -> epica_core::BeliefId {
    quad.insert(BeliefNode::new(
        key,
        BeliefValue::Inferred(serde_json::json!(key)),
        Provenance::LlmInference {
            model: "test".into(),
            call_id: uuid::Uuid::new_v4(),
            prompt_hash: 0,
        },
        confidence,
    ))
}

#[test]
fn system1_propagates_confidence_downstream() {
    let mut quad = BeliefQuad::new();

    let premise = insert(&mut quad, "premise", 0.9);
    let conclusion = insert(&mut quad, "conclusion", 0.5); // will be updated by System 1

    quad.add_causal_edge(
        premise,
        conclusion,
        CausalEdge::InferredFrom { premises: vec![premise] },
    );

    quad.propagate_system1(premise);

    let new_conf = quad.get(conclusion).unwrap().fast_confidence;
    // Noisy-OR of [0.9] = 0.9, decay = 1.0 (no TTL) → new_conf ≈ 0.9
    assert!(
        (new_conf - 0.9).abs() < 1e-4,
        "System 1 should propagate premise confidence to conclusion; got {new_conf}"
    );
}

#[test]
fn system1_noisy_or_with_two_premises() {
    let mut quad = BeliefQuad::new();

    let p1 = insert(&mut quad, "p1", 0.8);
    let p2 = insert(&mut quad, "p2", 0.7);
    let conclusion = insert(&mut quad, "conclusion", 0.0);

    // conclusion is inferred from BOTH p1 and p2
    quad.add_causal_edge(
        p1,
        conclusion,
        CausalEdge::InferredFrom { premises: vec![p1, p2] },
    );

    quad.propagate_system1(p1);

    let conf = quad.get(conclusion).unwrap().fast_confidence;
    // Noisy-OR([0.8, 0.7]) = 1 - (0.2)(0.3) = 1 - 0.06 = 0.94
    assert!(
        (conf - 0.94).abs() < 1e-4,
        "Noisy-OR of [0.8, 0.7] should be 0.94; got {conf}"
    );
}

#[test]
fn system1_does_not_cycle() {
    let mut quad = BeliefQuad::new();

    let a = insert(&mut quad, "a", 0.8);
    let b = insert(&mut quad, "b", 0.7);

    // Artificially create a cycle (A inferred from B, B inferred from A)
    quad.add_causal_edge(a, b, CausalEdge::InferredFrom { premises: vec![a] });
    quad.add_causal_edge(b, a, CausalEdge::InferredFrom { premises: vec![b] });

    // Must terminate (cycle guard via visited set)
    quad.propagate_system1(a);
    // If we reach here, the cycle guard worked
}
