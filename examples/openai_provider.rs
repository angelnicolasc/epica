//! System 2 reflection driven by the OpenAI provider.
//!
//! Demonstrates the full System 2 lifecycle when the runtime is wired to
//! [`OpenAiLlmClient`]:
//!
//!   1. `update_belief()` returns `System2Pending` because divergence
//!      exceeds tau (default 0.15).
//!   2. The caller (this example) consumes a budget token, calls the LLM,
//!      and applies the recalibrated confidence via
//!      `apply_system2_result()`.
//!   3. The node's `slow_confidence` reflects the LLM's revised estimate.
//!
//! Requires `OPENAI_API_KEY` in the environment. Without a real key the
//! example exits with a friendly diagnostic instead of crashing.
//!
//! Run with: `cargo run --example openai_provider --features openai`

use std::sync::Arc;

use epica_core::{BeliefNode, BeliefQuad, BeliefValue, Provenance};
use epica_openai::OpenAiLlmClient;
use epica_runtime::{BeliefRuntime, RuntimeUpdateResult};

#[tokio::main]
async fn main() {
    // ── Build a runtime with OpenAI as the System 2 backend ──────────────────
    let Ok(client) = OpenAiLlmClient::from_env() else {
        eprintln!(
            "OPENAI_API_KEY is not set — the example needs a live key to call \
             the Chat Completions API. Set OPENAI_API_KEY and re-run."
        );
        std::process::exit(0);
    };

    let rt = BeliefRuntime::new(BeliefQuad::new(), 0.5, 10, 1.0)
        .with_llm_client(Arc::new(client));

    // ── Insert a belief and revise it with a large divergence ────────────────
    let id = rt
        .insert_belief(BeliefNode::new(
            "tool_choice",
            BeliefValue::Asserted("call patch_file".into()),
            Provenance::UserStatement { turn: 0 },
            0.5,
        ))
        .await;

    // |0.95 - 0.5| = 0.45 ≫ tau 0.15 — System 2 is eligible.
    let result = rt
        .update_belief(
            id,
            BeliefValue::Asserted("call patch_file".into()),
            Provenance::LlmInference {
                model: "gpt-4o-mini".into(),
                call_id: uuid::Uuid::new_v4(),
                prompt_hash: 0xfeed_face,
            },
            0.95,
        )
        .await
        .expect("update_belief succeeds");

    let signal = match result {
        RuntimeUpdateResult::System2Pending { signal } => signal,
        other => {
            eprintln!("expected System2Pending, got {other:?}");
            std::process::exit(2);
        }
    };
    eprintln!(
        "runtime returned System2Pending: divergence = {:.4}",
        signal.divergence
    );

    // ── Simulate the MCP handler flow: budget → LLM → apply ──────────────────
    if !rt.try_consume_system2_budget().await {
        eprintln!("System 2 budget exhausted");
        std::process::exit(3);
    }

    let llm_client = rt
        .llm_client_arc()
        .expect("client was attached via with_llm_client");

    match llm_client.reflect(&signal).await {
        Ok(reflection) => {
            eprintln!(
                "OpenAI returned revised_confidence = {:.4}  ({} chars of reasoning)",
                reflection.revised_confidence,
                reflection.reasoning.chars().count()
            );
            rt.apply_system2_result(id, reflection.revised_confidence)
                .await;

            let quad = rt.read_quad().await;
            let node = quad.get(id).expect("node still present");
            eprintln!(
                "final state: fast={:.4} slow={:.4}",
                node.fast_confidence,
                node.slow_confidence.unwrap_or_default()
            );
        }
        Err(e) => {
            // Budget refund on transient failure keeps the session moving.
            rt.release_system2_budget().await;
            eprintln!("OpenAI call failed: {e}");
            std::process::exit(4);
        }
    }
}
