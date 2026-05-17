//! Codebase agent example: realistic use of BeliefQuad without the proc macro.
//!
//! Demonstrates:
//! - Typed beliefs with causal structure
//! - Checkpoint before risky operations
//! - System 1 confidence propagation
//! - Manual contract-like precondition check (Phase 3 will automate this)
//!
//! Run with: `cargo run --example codebase_agent`

use std::collections::HashSet;
use epica_core::{
    BeliefNode, BeliefQuad, BeliefValue, CausalEdge, Provenance, SemanticEdge,
};

struct CodebaseAgent {
    quad: BeliefQuad,
    // IDs for key beliefs
    file_tree_id: epica_core::BeliefId,
    user_intent_id: epica_core::BeliefId,
    blockers_id: epica_core::BeliefId,
    destructive_ops_id: epica_core::BeliefId,
}

impl CodebaseAgent {
    fn new() -> Self {
        let mut quad = BeliefQuad::new();

        let file_tree_id = quad.insert(BeliefNode::new(
            "file_tree",
            BeliefValue::Deterministic(serde_json::json!({
                "src/auth.rs": "auth module",
                "src/session.rs": "session store",
                "src/main.rs": "entrypoint"
            })),
            Provenance::ToolResult {
                tool: "list_files".into(),
                call_id: uuid::Uuid::new_v4(),
            },
            1.0,
        ));

        let user_intent_id = quad.insert(
            BeliefNode::new(
                "user_intent",
                BeliefValue::Inferred(serde_json::json!("refactor auth module")),
                Provenance::LlmInference {
                    model: "claude-sonnet-4-6".into(),
                    call_id: uuid::Uuid::new_v4(),
                    prompt_hash: 0,
                },
                0.85,
            )
            .with_reflection_threshold(0.15), // τ from paper AUQ
        );

        let blockers_id = quad.insert(BeliefNode::new(
            "blockers",
            BeliefValue::Inferred(serde_json::json!(["jwt_expiry_logic"])),
            Provenance::LlmInference {
                model: "claude-sonnet-4-6".into(),
                call_id: uuid::Uuid::new_v4(),
                prompt_hash: 1,
            },
            0.70,
        ));

        let destructive_ops_id = quad.insert(BeliefNode::new(
            "destructive_operations",
            BeliefValue::Deterministic(serde_json::json!([])),
            Provenance::RuntimeInference { derived_from: vec![] },
            1.0,
        ));

        // Causal structure: user_intent → blockers
        quad.add_causal_edge(
            user_intent_id,
            blockers_id,
            CausalEdge::InferredFrom { premises: vec![user_intent_id] },
        );

        // Semantic structure: file_tree and user_intent are related
        quad.add_semantic_edge(file_tree_id, user_intent_id, SemanticEdge::Synonymous);

        Self { quad, file_tree_id, user_intent_id, blockers_id, destructive_ops_id }
    }

    fn confidence_ok_to_act(&self) -> bool {
        let intent_conf = self.quad.get(self.user_intent_id).map(|n| n.fast_confidence).unwrap_or(0.0);
        intent_conf >= 0.70 // Phase 3: this becomes a contract precondition
    }

    fn plan_action(&mut self) {
        println!("Planning action...");

        // System 1: propagate from file_tree to intent
        self.quad.propagate_system1(self.file_tree_id);

        // Manual precondition check (Phase 3 automates via BehavioralContract)
        if !self.confidence_ok_to_act() {
            println!("  Insufficient confidence to act — aborting");
            return;
        }

        // Checkpoint before the risky write operation
        let cp = self.quad.checkpoint();
        println!("  Checkpoint saved: {:?}", cp);

        // Register a destructive operation
        let _ = self.quad.revise(
            self.destructive_ops_id,
            BeliefValue::Deterministic(serde_json::json!(["overwrite_auth.rs"])),
            Provenance::RuntimeInference { derived_from: vec![self.user_intent_id] },
            0.95,
        );

        // Simulate: a new tool result contradicts user_intent
        let new_result = self.quad.revise(
            self.user_intent_id,
            BeliefValue::Inferred(serde_json::json!("refactor session store (not auth)")),
            Provenance::ToolResult {
                tool: "code_analysis".into(),
                call_id: uuid::Uuid::new_v4(),
            },
            0.92,
        );

        match new_result {
            Ok(record) => {
                println!("  Revised intent. Contracted {} beliefs.", record.contracted.len());
                // Propagate new confidence
                self.quad.propagate_system1(self.user_intent_id);

                // Destructive_ops no longer applies — rollback to checkpoint
                let diff = self.quad.rollback_to(cp).expect("rollback should succeed");
                println!("  Rolled back. Diff modified: {}", diff.modified.len());
                println!("  Root cause: {:?}", diff.modified.iter().map(|m| &m.key).collect::<Vec<_>>());
            }
            Err(e) => println!("  Revision error: {e}"),
        }
    }
}

fn main() {
    let mut agent = CodebaseAgent::new();

    println!("=== Codebase Agent ===");
    println!(
        "Initial intent confidence: {:.3}",
        agent.quad.get(agent.user_intent_id).unwrap().fast_confidence
    );

    agent.plan_action();

    println!("\nFinal state:");
    for (_, node) in agent.quad.iter() {
        println!("  {} — confidence: {:.3}", node.key, node.fast_confidence);
    }
}
