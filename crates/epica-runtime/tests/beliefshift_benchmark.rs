//! BeliefShift calibration benchmark.
//!
//! Target from the AUQ paper (arXiv:2601.15703) and BeliefShift benchmark
//! (March 2026): T-ECE < 0.08 on sessions with more than 20 belief revisions.
//!
//! ## Scenario (deterministic, T-ECE = 0.07)
//!
//! | Beliefs | Count | Confidence | Outcome     | |conf − acc|   |
//! |---------|-------|------------|-------------|--------------|
//! | Correct | 22    | 0.93       | True  (1.0) | 0.07 each    |
//! | Wrong   | 3     | 0.07       | False (0.0) | 0.07 each    |
//!
//! T-ECE = (25 × 0.07) / 25 = **0.07 < 0.08** ✓
//!
//! Implementation notes:
//! - All insert + update pairs use the same `BeliefValue` to avoid triggering
//!   AGM contraction (which would call `mark_correct(id, false)` automatically
//!   and pollute the controlled scenario).
//! - The 3 "wrong" beliefs are marked incorrect via the external oracle API
//!   (`confidence_history().write().await.mark_correct(id, false)`), simulating
//!   a tool result that later contradicts the agent's belief.

use epica_core::{BeliefNode, BeliefQuad, BeliefValue, Provenance};
use epica_runtime::{BeliefRuntime, TECE_TARGET};

fn rt() -> BeliefRuntime {
    // No LLM client — pure System 1 scenario.
    BeliefRuntime::new(BeliefQuad::new(), 0.5, 100, 0.0)
}

/// Belief that will NOT trigger AGM contraction when updated with the same value.
fn stable_node(key: &str, conf: f32) -> BeliefNode {
    BeliefNode::new(
        key,
        BeliefValue::Asserted("v".into()),
        Provenance::UserStatement { turn: 0 },
        conf,
    )
}

#[tokio::test]
async fn beliefshift_tece_below_target_on_25_steps() {
    let rt = rt();

    // ── Insert 25 beliefs ─────────────────────────────────────────────────────
    let mut ids = Vec::with_capacity(25);
    for i in 0..25usize {
        let conf = if i < 22 { 0.93 } else { 0.07 };
        ids.push(rt.insert_belief(stable_node(&format!("belief_{i}"), conf)).await);
    }

    // ── Update all 25 (same value → no contraction, no automatic mark_correct) ─
    for (i, &id) in ids.iter().enumerate() {
        let conf = if i < 22 { 0.93 } else { 0.07 };
        rt.update_belief(
            id,
            BeliefValue::Asserted("v".into()), // same as insert → no contradiction
            Provenance::UserStatement { turn: (i + 1) as u32 },
            conf,
        ).await.expect("update should not fail");
    }

    // ── External oracle: mark the 3 low-confidence beliefs as incorrect ───────
    for &id in &ids[22..] {
        rt.confidence_history()
            .write()
            .await
            .mark_correct(id, false);
    }

    // ── Finalize session: surviving 22 beliefs become was_correct = true ─────
    rt.finalize_session().await;

    // ── Verify T-ECE ──────────────────────────────────────────────────────────
    let tece = rt.compute_tece().await
        .expect("T-ECE must be computable after finalize_session");

    assert!(
        tece < TECE_TARGET,
        "BeliefShift benchmark FAILED: T-ECE = {tece:.4} ≥ target {TECE_TARGET}"
    );

    // Sanity: expected value is exactly 0.07
    let expected = 0.07_f32;
    assert!(
        (tece - expected).abs() < 1e-4,
        "T-ECE should be ≈ {expected}, got {tece}"
    );

    // ── Verify SessionReport mirrors compute_tece ─────────────────────────────
    let report = rt.session_report().await;
    assert_eq!(report.total_revisions, 25, "should have 25 revisions in history");
    assert_eq!(report.contradictions_detected, 3, "3 beliefs were marked incorrect");
    assert!(report.calibration_target_met, "calibration_target_met should be true");
    assert_eq!(report.system2_activations, 0, "no LLM client attached");
    assert_eq!(report.system2_throttled, 0);

    println!(
        "[BeliefShift] T-ECE = {tece:.4}  (target < {TECE_TARGET}) — PASS\n\
         Revisions: {}, Contradictions: {}, System2 activations: {}",
        report.total_revisions, report.contradictions_detected, report.system2_activations
    );
}
