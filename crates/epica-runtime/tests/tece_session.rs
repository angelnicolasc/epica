//! Integration tests: Trajectory-ECE computation over a session.

use std::sync::Arc;

use async_trait::async_trait;
use epica_core::{BeliefNode, BeliefQuad, BeliefValue, Provenance};
use epica_runtime::{
    BeliefRuntime, DiagnosticSignal, LlmClient, LlmClientError, RuntimeUpdateResult, System2Result,
};

fn rt() -> BeliefRuntime {
    BeliefRuntime::new(BeliefQuad::new(), 0.5, 100, 0.0)
}

fn asserted(key: &str, conf: f32) -> BeliefNode {
    BeliefNode::new(
        key,
        BeliefValue::Asserted("v".into()),
        Provenance::UserStatement { turn: 0 },
        conf,
    )
}

// ── 1. No T-ECE before any known outcomes ─────────────────────────────────────

#[tokio::test]
async fn tece_none_on_empty_history() {
    let rt = rt();
    assert!(rt.compute_tece().await.is_none());
    // After inserting and updating, still no known outcomes → still None.
    let id = rt.insert_belief(asserted("k", 0.8)).await;
    rt.update_belief(
        id,
        BeliefValue::Asserted("v".into()),
        Provenance::UserStatement { turn: 1 },
        0.8,
    ).await.unwrap();
    assert!(rt.compute_tece().await.is_none());
}

// ── 2. T-ECE reflects contradictions ─────────────────────────────────────────

#[tokio::test]
async fn tece_reflects_single_contradiction() {
    let rt = rt();

    let id = rt.insert_belief(asserted("sensor", 0.8)).await;

    // Record a revision so the history has an entry with confidence=0.8.
    rt.update_belief(
        id,
        BeliefValue::Asserted("v".into()),
        Provenance::UserStatement { turn: 1 },
        0.8,
    ).await.unwrap();

    // External oracle marks this belief incorrect (e.g., a tool result contradicted it).
    rt.confidence_history()
        .write()
        .await
        .mark_correct(id, false);

    // At least one record now has was_correct=false → T-ECE is computable.
    let tece = rt.compute_tece().await.expect("should have at least one known outcome");
    // |0.8 − 0.0| = 0.8 for the incorrect record.
    assert!(tece > 0.0, "T-ECE should be positive after a contradiction");
    assert!((tece - 0.8).abs() < 1e-4, "expected T-ECE ≈ 0.8, got {tece}");
}

// ── 3. finalize_session marks survivors as correct ───────────────────────────

#[tokio::test]
async fn finalize_session_populates_tece_from_surviving_beliefs() {
    let rt = rt();
    let id = rt.insert_belief(asserted("k", 0.9)).await;
    rt.update_belief(
        id,
        BeliefValue::Asserted("v".into()),
        Provenance::UserStatement { turn: 1 },
        0.9,
    ).await.unwrap();

    // Before finalize: no known outcomes.
    assert!(rt.compute_tece().await.is_none());

    rt.finalize_session().await;

    // After finalize: surviving belief marked correct → T-ECE computable.
    let tece = rt.compute_tece().await.expect("T-ECE should be available after finalize");
    // |0.9 - 1.0| = 0.1
    assert!((tece - 0.1).abs() < 1e-4, "expected T-ECE ≈ 0.1, got {tece}");
}

// ── 4. System 2 revised confidence is what the history records ────────────────

struct FixedClient(f32);

#[async_trait]
impl LlmClient for FixedClient {
    async fn reflect(&self, _: &DiagnosticSignal) -> Result<System2Result, LlmClientError> {
        Ok(System2Result { revised_confidence: self.0, reasoning: "fixed".into() })
    }
}

#[tokio::test]
async fn history_records_system2_revised_confidence() {
    let revised = 0.95_f32;
    // reliability_baseline=0.5, τ default=0.15 → divergence must exceed 0.15.
    let rt = BeliefRuntime::new(BeliefQuad::new(), 0.5, 10, 0.0)
        .with_llm_client(Arc::new(FixedClient(revised)));

    let id = rt.insert_belief(asserted("k", 0.5)).await;

    // fast=0.9 → divergence 0.4 > 0.15 → System 2 eligible → System2Pending.
    // S-2 fix: update_belief() returns Pending; the LLM is not called inline.
    let result = rt.update_belief(
        id,
        BeliefValue::Asserted("v".into()),
        Provenance::UserStatement { turn: 1 },
        0.9,
    ).await.unwrap();

    // Simulate handler flow: consume budget → call LLM → apply result.
    // apply_system2_result() *replaces* (not appends) the fast-confidence entry,
    // so the history contains one data point at 0.95 rather than two at 0.9 + 0.95.
    let signal = match result {
        RuntimeUpdateResult::System2Pending { signal } => signal,
        _ => panic!("expected System2Pending, got {result:?}"),
    };
    let consumed = rt.try_consume_system2_budget().await;
    assert!(consumed, "budget should be available");
    let llm_result = rt.llm_client_arc().unwrap().reflect(&signal).await.unwrap();
    rt.apply_system2_result(id, llm_result.revised_confidence).await;

    rt.finalize_session().await;
    let tece = rt.compute_tece().await.expect("should be computable");
    // History has one entry: revised=0.95, was_correct=true (after finalize).
    // T-ECE = |0.95 - 1.0| = 0.05
    assert!(
        (tece - 0.05).abs() < 1e-4,
        "expected T-ECE ≈ 0.05 (System 2 revised confidence recorded), got {tece}"
    );
}
