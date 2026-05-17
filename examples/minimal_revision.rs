//! Minimal example: insert → revise → System 1 propagation → diff.
//!
//! Run with: `cargo run --example minimal_revision`

use epica_core::{BeliefNode, BeliefQuad, BeliefValue, CausalEdge, Provenance};

fn main() {
    let mut quad = BeliefQuad::new();

    // Insert a root belief: the agent's understanding of user intent
    let intent_id = quad.insert(
        BeliefNode::new(
            "user_intent",
            BeliefValue::Inferred(serde_json::json!("refactor auth module")),
            Provenance::LlmInference {
                model: "claude-sonnet-4-6".into(),
                call_id: uuid::Uuid::new_v4(),
                prompt_hash: 0xdeadbeef,
            },
            0.85,
        )
        .with_ttl_ms(600_000), // 10 minutes TTL
    );

    // Insert a derived belief: inferred blockers
    let blocker_id = quad.insert(BeliefNode::new(
        "blockers",
        BeliefValue::Inferred(serde_json::json!(["jwt_expiry_logic", "session_store"])),
        Provenance::LlmInference {
            model: "claude-sonnet-4-6".into(),
            call_id: uuid::Uuid::new_v4(),
            prompt_hash: 0xcafebabe,
        },
        0.70,
    ));

    // Link: blockers were inferred from user_intent
    quad.add_causal_edge(
        intent_id,
        blocker_id,
        CausalEdge::InferredFrom { premises: vec![intent_id] },
    );

    println!("Before revision:");
    println!("  user_intent confidence:  {:.3}", quad.get(intent_id).unwrap().fast_confidence);
    println!("  blockers confidence:     {:.3}", quad.get(blocker_id).unwrap().fast_confidence);

    // Take a checkpoint before the revision
    let checkpoint = quad.checkpoint();

    // Revise user_intent with stronger evidence
    let record = quad
        .revise(
            intent_id,
            BeliefValue::Inferred(serde_json::json!("refactor auth + session modules")),
            Provenance::LlmInference {
                model: "claude-sonnet-4-6".into(),
                call_id: uuid::Uuid::new_v4(),
                prompt_hash: 0x12345678,
            },
            0.95,
        )
        .expect("revision should succeed");

    // System 1: propagate the new confidence downstream
    quad.propagate_system1(intent_id);

    println!("\nAfter revision (System 1 propagated):");
    println!("  user_intent confidence:  {:.3}", quad.get(intent_id).unwrap().fast_confidence);
    println!("  blockers confidence:     {:.3}", quad.get(blocker_id).unwrap().fast_confidence);
    println!("  Revision was expand-only: {}", record.is_expansion());
    println!("  K*2 success:   {}", record.postulate_audit.success);
    println!("  K*4 vacuity:   {}", record.postulate_audit.vacuity);
    println!("  K*5 consistency: {}", record.postulate_audit.consistency);

    // Counterfactual: what if intent had never existed?
    let cf = quad.counterfactual_query(intent_id);
    println!("\nCounterfactual (intent removed):");
    println!("  Excluded beliefs: {}", cf.excluded.len());
    println!("  Surviving beliefs: {}", cf.surviving_count());

    // Rollback to the pre-revision checkpoint
    let diff = quad.rollback_to(checkpoint).expect("rollback should succeed");
    println!("\nAfter rollback:");
    println!("  Diff has contradictions: {}", diff.has_contradictions());
    println!("  Modified beliefs: {}", diff.modified.len());
    println!("  user_intent value: {:?}", quad.get(intent_id).map(|n| &n.value));
}
