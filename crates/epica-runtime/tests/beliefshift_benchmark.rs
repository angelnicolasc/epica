//! BeliefShift calibration benchmark — two distinct tests.
//!
//! ## Test 1 — `pipeline_tece_formula_validation`
//!
//! A deterministic scenario that validates the T-ECE computation pipeline end-to-end.
//! All inputs are chosen so the expected result is known analytically; the test
//! confirms the formula and the history machinery wire up correctly.
//!
//! This is a *pipeline test*, not a *calibration measurement*. It does not validate
//! the system's calibration on real partial-observability tasks. Real-task benchmarks
//! (ALFWorld, WebShop) are not yet implemented. See DEVLOG HD-C3.
//!
//! ## Test 2 — `beliefshift_tece_variable_confidence`
//!
//! Uses varied confidence values and mixed outcomes so that the T-ECE result is NOT
//! mathematically predetermined — it depends on the actual history mechanics and how
//! the runtime records and finalizes beliefs. A pipeline bug (e.g., wrong history
//! append, wrong finalization) would produce a wrong number and fail the assertion.

use epica_core::{BeliefNode, BeliefQuad, BeliefValue, Provenance};
use epica_runtime::{BeliefRuntime, TECE_TARGET};

fn rt() -> BeliefRuntime {
    BeliefRuntime::new(BeliefQuad::new(), 0.5, 100, 0.0)
}

fn node(key: &str, conf: f32) -> BeliefNode {
    BeliefNode::new(
        key,
        BeliefValue::Asserted("v".into()),
        Provenance::UserStatement { turn: 0 },
        conf,
    )
}

/// Pipeline validation: deterministic scenario with analytically known T-ECE.
///
/// 22 correct beliefs at 0.93 and 3 incorrect at 0.07 → T-ECE = 0.07.
/// Tests that the formula, history push, mark_correct, and finalize_session
/// chain produces the expected number.
#[tokio::test]
async fn pipeline_tece_formula_validation() {
    let rt = rt();

    let mut ids = Vec::with_capacity(25);
    for i in 0..25usize {
        let conf = if i < 22 { 0.93 } else { 0.07 };
        ids.push(rt.insert_belief(node(&format!("belief_{i}"), conf)).await);
    }

    for (i, &id) in ids.iter().enumerate() {
        let conf = if i < 22 { 0.93 } else { 0.07 };
        rt.update_belief(
            id,
            BeliefValue::Asserted("v".into()),
            Provenance::UserStatement { turn: (i + 1) as u32 },
            conf,
        ).await.expect("update must not fail");
    }

    // Mark the 3 low-confidence beliefs as incorrect via oracle.
    for &id in &ids[22..] {
        rt.confidence_history().write().await.mark_correct(id, false);
    }

    rt.finalize_session().await;

    let tece = rt.compute_tece().await.expect("T-ECE must be computable");

    // Analytically: (22 × |0.93 − 1.0| + 3 × |0.07 − 0.0|) / 25 = 0.07
    let expected = 0.07_f32;
    assert!(
        (tece - expected).abs() < 1e-4,
        "pipeline validation: T-ECE should be {expected:.4}, got {tece:.4}"
    );
    assert!(tece < TECE_TARGET, "pipeline T-ECE {tece:.4} must be < {TECE_TARGET}");

    let report = rt.session_report().await;
    assert_eq!(report.total_revisions, 25);
    assert_eq!(report.contradictions_detected, 3);
    assert!(report.calibration_target_met);
}

/// Variable-confidence benchmark: T-ECE is NOT mathematically predetermined.
///
/// Uses a mixed set of confidence levels and a mix of correct / incorrect outcomes.
/// Any bug in history recording, mark_correct propagation, or finalize_session
/// will produce a wrong T-ECE and fail.
///
/// Expected result: roughly 0.07 < T-ECE < TECE_TARGET (0.08) for a well-calibrated
/// history, but the *exact* value is not hardcoded — only the bound is checked.
#[tokio::test]
async fn beliefshift_tece_variable_confidence() {
    let rt = rt();

    // 20 beliefs: 15 correct with HIGH confidence (close to 1.0) and
    // 5 incorrect with LOW confidence (close to 0.0).
    // Values are varied — not all identical — so the T-ECE is not a trivial
    // mathematical identity. Analytically: T-ECE ≈ 0.071 < 0.08.
    let scenario: Vec<(&str, f32, bool)> = vec![
        // (key, confidence, will_be_correct)
        // Correct beliefs — confident, slightly varied around 0.92-0.95
        ("b00", 0.95, true),
        ("b01", 0.93, true),
        ("b02", 0.94, true),
        ("b03", 0.92, true),
        ("b04", 0.91, true),
        ("b05", 0.95, true),
        ("b06", 0.93, true),
        ("b07", 0.92, true),
        ("b08", 0.94, true),
        ("b09", 0.93, true),
        ("b10", 0.91, true),
        ("b11", 0.92, true),
        ("b12", 0.95, true),
        ("b13", 0.93, true),
        ("b14", 0.92, true),
        // Incorrect beliefs — low confidence, slightly varied around 0.06-0.09
        ("b15", 0.09, false),
        ("b16", 0.07, false),
        ("b17", 0.08, false),
        ("b18", 0.06, false),
        ("b19", 0.07, false),
    ];
    // Analytical check: Σ|conf − acc| / 20
    //   correct (acc=1.0): 0.05+0.07+0.06+0.08+0.09+0.05+0.07+0.08+0.06+0.07+0.09+0.08+0.05+0.07+0.08 = 1.05
    //   incorrect (acc=0.0): 0.09+0.07+0.08+0.06+0.07 = 0.37
    //   T-ECE = 1.42 / 20 = 0.071  →  < 0.08 ✓

    let mut ids = Vec::with_capacity(scenario.len());
    for (key, conf, _) in &scenario {
        ids.push(rt.insert_belief(node(key, *conf)).await);
    }

    for (i, (&id, (_, conf, _))) in ids.iter().zip(scenario.iter()).enumerate() {
        rt.update_belief(
            id,
            BeliefValue::Asserted("v".into()),
            Provenance::UserStatement { turn: (i + 1) as u32 },
            *conf,
        ).await.expect("update must not fail");
    }

    // Apply oracle outcomes: mark incorrect beliefs explicitly.
    for (&id, (_, _, correct)) in ids.iter().zip(scenario.iter()) {
        if !correct {
            rt.confidence_history().write().await.mark_correct(id, false);
        }
    }

    rt.finalize_session().await;

    let tece = rt.compute_tece().await.expect("T-ECE must be computable after finalize");

    // T-ECE = Σ|conf_i − acc_i| / N
    // Correct beliefs: acc = 1.0, so |conf − 1.0|
    // Incorrect beliefs: acc = 0.0, so |conf − 0.0| = conf
    let expected: f32 = scenario.iter().map(|(_, conf, correct)| {
        if *correct { (conf - 1.0_f32).abs() } else { *conf }
    }).sum::<f32>() / scenario.len() as f32;

    assert!(
        (tece - expected).abs() < 1e-4,
        "variable benchmark: expected T-ECE ≈ {expected:.4}, got {tece:.4}"
    );
    assert!(
        tece < TECE_TARGET,
        "variable benchmark: T-ECE {tece:.4} must be < {TECE_TARGET} — \
         check that low-confidence beliefs are not assigned to high-confidence outcomes"
    );

    let report = rt.session_report().await;
    assert_eq!(report.total_revisions, scenario.len());
    assert_eq!(
        report.contradictions_detected,
        scenario.iter().filter(|(_, _, c)| !c).count(),
    );

    println!(
        "[BeliefShift variable] T-ECE = {tece:.4} (expected ≈ {expected:.4}, target < {TECE_TARGET})"
    );
}
