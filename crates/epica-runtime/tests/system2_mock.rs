//! Integration tests: System 2 activation using a deterministic `MockLlmClient`.

use std::sync::Arc;

use async_trait::async_trait;
use epica_core::{BeliefNode, BeliefQuad, BeliefValue, Provenance};
use epica_runtime::{
    BeliefRuntime, DiagnosticSignal, LlmClient, LlmClientError, RuntimeUpdateResult, System2Result,
};

// ── MockLlmClient ─────────────────────────────────────────────────────────────

/// Deterministic LLM client for tests.  Always returns the same revised confidence.
struct MockLlmClient {
    revised_confidence: f32,
}

#[async_trait]
impl LlmClient for MockLlmClient {
    async fn reflect(
        &self,
        _diagnostic: &DiagnosticSignal,
    ) -> Result<System2Result, LlmClientError> {
        Ok(System2Result {
            revised_confidence: self.revised_confidence,
            reasoning: "mock recalibration".into(),
        })
    }
}

fn mock_rt(budget: u32, revised: f32) -> BeliefRuntime {
    BeliefRuntime::new(BeliefQuad::new(), 0.5, budget, 0.0)
        .with_llm_client(Arc::new(MockLlmClient { revised_confidence: revised }))
}

fn node(key: &str, conf: f32) -> BeliefNode {
    BeliefNode::new(
        key,
        BeliefValue::Asserted("v".into()),
        Provenance::UserStatement { turn: 0 },
        conf,
    )
}

// ── 1. Activation on high divergence ─────────────────────────────────────────

/// confidence 0.9 → divergence |0.9-0.5| = 0.4 > τ 0.15 → System 2 fires.
///
/// S-2 fix: `update_belief()` returns `System2Pending`; the caller (MCP handler)
/// is responsible for consuming the budget, calling the LLM, and applying the
/// result. This test simulates that handler flow directly.
#[tokio::test]
async fn system2_activates_on_high_divergence() {
    let rt = mock_rt(10, 0.95);
    let id = rt.insert_belief(node("k", 0.5)).await;
    let result = rt.update_belief(
        id,
        BeliefValue::Asserted("v".into()),
        Provenance::UserStatement { turn: 1 },
        0.9,
    ).await.unwrap();

    // Step 1: runtime returns Pending — LLM call is NOT made yet.
    let signal = match result {
        RuntimeUpdateResult::System2Pending { signal } => signal,
        _ => panic!("expected System2Pending, got {result:?}"),
    };

    // Step 2: simulate handler — consume budget, call LLM, apply result.
    let consumed = rt.try_consume_system2_budget().await;
    assert!(consumed, "budget should be available");
    let llm_result = rt.llm_client_arc().unwrap().reflect(&signal).await.unwrap();
    rt.apply_system2_result(id, llm_result.revised_confidence).await;

    assert_eq!(rt.system2_activations(), 1);
}

// ── 2. No activation below τ ──────────────────────────────────────────────────

/// confidence 0.57 → divergence |0.57-0.5| = 0.07 < τ 0.15 → System 1 only.
#[tokio::test]
async fn system2_does_not_activate_below_tau() {
    let rt = mock_rt(10, 0.95);
    let id = rt.insert_belief(node("k", 0.5)).await;
    let result = rt.update_belief(
        id,
        BeliefValue::Asserted("v".into()),
        Provenance::UserStatement { turn: 1 },
        0.57,
    ).await.unwrap();
    assert!(
        matches!(result, RuntimeUpdateResult::System1Only),
        "expected System1Only, got {result:?}"
    );
    assert_eq!(rt.system2_activations(), 0);
}

// ── 3. Throttling when budget is zero ─────────────────────────────────────────

/// S-2 fix: budget is NOT consumed inside `update_belief()`. The runtime always
/// returns `System2Pending` when the divergence threshold is exceeded and an LLM
/// client is attached. Budget enforcement is the caller's (MCP handler's)
/// responsibility. This test simulates that: handler checks budget, finds it
/// empty, records the throttle, and skips the LLM call.
#[tokio::test]
async fn system2_throttled_when_budget_exhausted() {
    let rt = mock_rt(0, 0.95); // budget = 0
    let id = rt.insert_belief(node("k", 0.5)).await;
    let result = rt.update_belief(
        id,
        BeliefValue::Asserted("v".into()),
        Provenance::UserStatement { turn: 1 },
        0.9,
    ).await.unwrap();

    // Runtime returns Pending regardless of budget — budget check is caller's job.
    assert!(
        matches!(result, RuntimeUpdateResult::System2Pending { .. }),
        "expected System2Pending, got {result:?}"
    );

    // Simulate handler: try to consume budget — budget is 0, returns false.
    let consumed = rt.try_consume_system2_budget().await;
    assert!(!consumed, "budget should be exhausted");

    // Handler records the throttle and skips the LLM call.
    rt.record_system2_throttle().await;

    assert_eq!(rt.system2_throttled(), 1);
    assert_eq!(rt.system2_activations(), 0);
}

// ── 4. slow_confidence written to node after activation ───────────────────────

/// S-2 fix: `slow_confidence` is written by `apply_system2_result()`, not by
/// `update_belief()`. This test simulates the full handler flow to confirm the
/// write reaches the node.
#[tokio::test]
async fn system2_writes_slow_confidence_to_node() {
    let revised = 0.77_f32;
    let rt = mock_rt(10, revised);
    let id = rt.insert_belief(node("k", 0.5)).await;
    let result = rt.update_belief(
        id,
        BeliefValue::Asserted("v".into()),
        Provenance::UserStatement { turn: 1 },
        0.9,
    ).await.unwrap();

    // Simulate handler: consume budget, call LLM, apply result.
    let signal = match result {
        RuntimeUpdateResult::System2Pending { signal } => signal,
        _ => panic!("expected System2Pending, got {result:?}"),
    };
    let consumed = rt.try_consume_system2_budget().await;
    assert!(consumed, "budget should be available");
    let llm_result = rt.llm_client_arc().unwrap().reflect(&signal).await.unwrap();
    rt.apply_system2_result(id, llm_result.revised_confidence).await;

    let quad = rt.read_quad().await;
    let slow = quad.get(id)
        .and_then(|n| n.slow_confidence)
        .expect("slow_confidence should be set after apply_system2_result");
    assert!(
        (slow - revised).abs() < 1e-5,
        "slow_confidence {slow} != expected {revised}"
    );
}
