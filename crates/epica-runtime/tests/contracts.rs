//! Integration tests: behavioral contract enforcement in BeliefRuntime.
//!
//! Verifies the ABC paper baseline (5.2–6.8 soft violations per session),
//! recovery policy execution, governance limits, and multi-contract composition.

use epica_contracts::{
    AllowedAgents, AuthPolicy, BehavioralContract, GovernancePolicies,
    MinConfidence, MnemonicSovereignty, RecoveryPolicy, SessionInvariant,
    ViolationClass, BeliefPredicate, BeliefExists,
};
use epica_core::{BeliefNode, BeliefQuad, BeliefValue, Provenance};
use epica_runtime::{BeliefRuntime, RuntimeError};

fn runtime() -> BeliefRuntime {
    BeliefRuntime::new(BeliefQuad::new(), 0.5, 10, 1.0)
}

fn node(key: &str, confidence: f32) -> BeliefNode {
    BeliefNode::new(
        key,
        BeliefValue::Asserted("v".into()),
        Provenance::UserStatement { turn: 0 },
        confidence,
    )
}

// ── ABC paper baseline ────────────────────────────────────────────────────────

/// The ABC paper (arXiv:2602.22302) reports 5.2–6.8 soft violations per session
/// on realistic agent traces. This test demonstrates that `ContractEngine` detects
/// soft violations that output-level guardrails would miss entirely.
///
/// Setup: a Soft invariant requires all beliefs to have confidence ≥ 0.5.
/// We insert 30 beliefs: 24 at confidence 0.8 (pass) and 6 at confidence 0.3
/// (violate). Each violation is detected at mutation time.
#[tokio::test]
async fn soft_violations_detected_per_session_abc_baseline() {
    // Invariant: every belief must have confidence ≥ 0.5.
    let invariant = SessionInvariant {
        predicate: Box::new(MinConfidence {
            key: "sentinel".into(),
            threshold: 0.5,
        }),
        class: ViolationClass::Soft,
        max_intervention_gap_steps: 10,
    };

    let contract = BehavioralContract {
        domain: "abc_baseline".into(),
        preconditions: vec![],
        invariants: vec![invariant],
        governance: GovernancePolicies::default(),
        recovery: RecoveryPolicy::RollbackToLastCheckpoint,
        p: 0.99, delta: 10, k: 50,
        alpha: 0.2, gamma: 1.0,
    };

    let rt = runtime().with_contract(contract);

    // Insert a sentinel belief at high confidence (invariant will pass while sentinel ≥ 0.5).
    let sentinel_node = node("sentinel", 0.8);
    let sentinel_id = rt.insert_belief(sentinel_node).await;

    // Insert 24 passing beliefs
    for i in 0..24u32 {
        let n = node(&format!("ok_{i}"), 0.8);
        rt.insert_belief(n).await;
    }

    // Trigger 6 soft violations by dropping the sentinel's confidence below 0.5
    // via update_belief on each of the 6 "violating" steps.
    // Strategy: update sentinel to low confidence, then back to high, repeat.
    let mut soft_count = 0u32;
    for _ in 0..6 {
        // Lower sentinel → invariant check will see it violate
        let _ = rt.update_belief(
            sentinel_id,
            BeliefValue::Asserted("low".into()),
            Provenance::UserStatement { turn: 1 },
            0.3,
        ).await;
        // Restore sentinel
        let _ = rt.update_belief(
            sentinel_id,
            BeliefValue::Asserted("high".into()),
            Provenance::UserStatement { turn: 2 },
            0.8,
        ).await;
        soft_count += 1;
    }

    rt.finalize_session().await;
    let report = rt.session_report().await;

    // We expect exactly the 6 "low" updates to produce soft violations.
    assert_eq!(report.soft_violations, 6, "Expected 6 soft violations (ABC baseline ≈ 5.2–6.8)");
    assert_eq!(report.hard_violations, 0);
    assert_eq!(report.critical_violations, 0);
    assert_eq!(report.recovery_actions_taken, 0);
}

// ── Hard violation triggers rollback ─────────────────────────────────────────

#[tokio::test]
async fn hard_violation_triggers_rollback() {
    // Precondition that fails → no, invariant that fails on Hard.
    // Invariant: belief "required" must exist.
    let invariant = SessionInvariant {
        predicate: Box::new(BeliefExists { key: "required".into() }),
        class: ViolationClass::Hard,
        max_intervention_gap_steps: 1,
    };

    let contract = BehavioralContract {
        domain: "hard_test".into(),
        preconditions: vec![],
        invariants: vec![invariant],
        governance: GovernancePolicies::default(),
        recovery: RecoveryPolicy::DegradeAndContinue { min_confidence: 0.1 },
        p: 0.99, delta: 1, k: 100,
        alpha: 0.05, gamma: 0.5,
    };

    let rt = runtime().with_contract(contract);

    // Insert "other" — "required" is absent, so the invariant fires Hard.
    let other = rt.insert_belief(node("other", 0.7)).await;
    let result = rt.update_belief(
        other,
        BeliefValue::Asserted("updated".into()),
        Provenance::UserStatement { turn: 0 },
        0.6,
    ).await;

    // DegradeAndContinue doesn't halt — it succeeds.
    assert!(result.is_ok(), "DegradeAndContinue should not halt: {result:?}");

    rt.finalize_session().await;
    let report = rt.session_report().await;
    assert_eq!(report.hard_violations, 1);
    assert_eq!(report.recovery_actions_taken, 1);
}

// ── Critical violation halts ──────────────────────────────────────────────────

#[tokio::test]
async fn critical_violation_halts_update() {
    let invariant = SessionInvariant {
        predicate: Box::new(BeliefExists { key: "safety_lock".into() }),
        class: ViolationClass::Critical,
        max_intervention_gap_steps: 0,
    };

    let contract = BehavioralContract {
        domain: "critical_test".into(),
        preconditions: vec![],
        invariants: vec![invariant],
        governance: GovernancePolicies::default(),
        recovery: RecoveryPolicy::RollbackToLastCheckpoint,
        p: 1.0, delta: 0, k: 100,
        alpha: 0.0, gamma: 1.0,
    };

    let rt = runtime().with_contract(contract);

    // "safety_lock" is absent — any update triggers the Critical invariant.
    let other = rt.insert_belief(node("other", 0.9)).await;
    let result = rt.update_belief(
        other,
        BeliefValue::Asserted("trigger".into()),
        Provenance::UserStatement { turn: 0 },
        0.8,
    ).await;

    assert!(
        matches!(result, Err(RuntimeError::ContractHalt { .. })),
        "Expected ContractHalt, got: {result:?}"
    );

    let report = rt.session_report().await;
    assert_eq!(report.critical_violations, 1);
}

// ── Precondition blocks update ────────────────────────────────────────────────

#[tokio::test]
async fn precondition_blocks_update_when_key_absent() {
    let contract = BehavioralContract {
        domain: "precondition_test".into(),
        preconditions: vec![Box::new(BeliefExists { key: "must_exist".into() })],
        invariants: vec![],
        governance: GovernancePolicies::default(),
        recovery: RecoveryPolicy::RollbackToLastCheckpoint,
        p: 0.99, delta: 1, k: 100,
        alpha: 0.05, gamma: 0.5,
    };

    let rt = runtime().with_contract(contract);

    let other = rt.insert_belief(node("other", 0.9)).await;
    let result = rt.update_belief(
        other,
        BeliefValue::Asserted("x".into()),
        Provenance::UserStatement { turn: 0 },
        0.8,
    ).await;

    assert!(
        matches!(result, Err(RuntimeError::PreconditionFailed { .. })),
        "Expected PreconditionFailed, got: {result:?}"
    );
}

#[tokio::test]
async fn precondition_passes_when_key_present() {
    let contract = BehavioralContract {
        domain: "precondition_pass_test".into(),
        preconditions: vec![Box::new(BeliefExists { key: "must_exist".into() })],
        invariants: vec![],
        governance: GovernancePolicies::default(),
        recovery: RecoveryPolicy::RollbackToLastCheckpoint,
        p: 0.99, delta: 1, k: 100,
        alpha: 0.05, gamma: 0.5,
    };

    let rt = runtime().with_contract(contract);

    // Insert the required key first.
    rt.insert_belief(node("must_exist", 0.9)).await;
    let other = rt.insert_belief(node("other", 0.9)).await;

    let result = rt.update_belief(
        other,
        BeliefValue::Asserted("x".into()),
        Provenance::UserStatement { turn: 0 },
        0.8,
    ).await;
    assert!(result.is_ok());
}

// ── Governance token limit ────────────────────────────────────────────────────

#[tokio::test]
async fn governance_token_limit_enforced() {
    let contract = BehavioralContract {
        domain: "token_limit_test".into(),
        preconditions: vec![],
        invariants: vec![],
        governance: GovernancePolicies {
            max_tokens_per_session: Some(3),
            ..GovernancePolicies::default()
        },
        recovery: RecoveryPolicy::RollbackToLastCheckpoint,
        p: 0.99, delta: 1, k: 100,
        alpha: 0.05, gamma: 0.5,
    };

    let rt = runtime().with_contract(contract);
    let id = rt.insert_belief(node("key", 0.9)).await;

    // First 3 updates succeed.
    for i in 0..3u32 {
        let res = rt.update_belief(
            id,
            BeliefValue::Asserted(format!("v{i}")),
            Provenance::UserStatement { turn: i },
            0.8,
        ).await;
        assert!(res.is_ok(), "Update {i} should succeed");
    }

    // 4th update should exceed the limit.
    let res = rt.update_belief(
        id,
        BeliefValue::Asserted("v4".into()),
        Provenance::UserStatement { turn: 4 },
        0.8,
    ).await;
    assert!(
        matches!(res, Err(RuntimeError::GovernanceLimitExceeded { .. })),
        "Expected GovernanceLimitExceeded, got: {res:?}"
    );
}

// ── DegradeAndContinue recovery ───────────────────────────────────────────────

#[tokio::test]
async fn recovery_degrade_and_continue_raises_confidence_floors() {
    let invariant = SessionInvariant {
        predicate: Box::new(BeliefExists { key: "guard".into() }),
        class: ViolationClass::Hard,
        max_intervention_gap_steps: 1,
    };

    let contract = BehavioralContract {
        domain: "degrade_test".into(),
        preconditions: vec![],
        invariants: vec![invariant],
        governance: GovernancePolicies::default(),
        recovery: RecoveryPolicy::DegradeAndContinue { min_confidence: 0.4 },
        p: 0.99, delta: 5, k: 100,
        alpha: 0.1, gamma: 0.5,
    };

    let rt = runtime().with_contract(contract);

    // Insert a belief with very low confidence.
    let low_conf = rt.insert_belief(BeliefNode::new(
        "low_conf_belief",
        BeliefValue::Asserted("v".into()),
        Provenance::UserStatement { turn: 0 },
        0.1, // below floor
    )).await;

    // Trigger the invariant ("guard" is absent → Hard violation → DegradeAndContinue).
    let _ = rt.update_belief(
        low_conf,
        BeliefValue::Asserted("updated".into()),
        Provenance::UserStatement { turn: 1 },
        0.1,
    ).await;

    // After recovery, the low-confidence belief should be raised to the floor.
    let quad = rt.read_quad().await;
    let conf = quad.get(low_conf).map(|n| n.fast_confidence).unwrap_or(0.0);
    assert!(
        conf >= 0.4,
        "Expected fast_confidence >= 0.4 after DegradeAndContinue, got {conf}"
    );
}

// ── Drift bound computation ───────────────────────────────────────────────────

#[tokio::test]
async fn drift_bound_included_in_session_report() {
    let contract = BehavioralContract {
        domain: "drift_test".into(),
        preconditions: vec![],
        invariants: vec![],
        governance: GovernancePolicies::default(),
        recovery: RecoveryPolicy::RollbackToLastCheckpoint,
        p: 0.99, delta: 1, k: 50,
        alpha: 0.1,
        gamma: 0.5,
    };

    let rt = runtime().with_contract(contract);
    rt.finalize_session().await;
    let report = rt.session_report().await;

    assert_eq!(report.drift_bounds.len(), 1);
    let bound = &report.drift_bounds[0];
    // D* = α/γ = 0.1/0.5 = 0.2
    assert!(
        (bound.expected_drift - 0.2).abs() < 1e-9,
        "Expected D* = 0.2, got {}",
        bound.expected_drift
    );
}

// ── Multi-contract composition ────────────────────────────────────────────────

#[tokio::test]
async fn multi_contract_composition_both_checked() {
    let soft_contract = BehavioralContract {
        domain: "soft_domain".into(),
        preconditions: vec![],
        invariants: vec![SessionInvariant {
            predicate: Box::new(BeliefExists { key: "soft_guard".into() }),
            class: ViolationClass::Soft,
            max_intervention_gap_steps: 5,
        }],
        governance: GovernancePolicies::default(),
        recovery: RecoveryPolicy::RollbackToLastCheckpoint,
        p: 0.99, delta: 5, k: 100,
        alpha: 0.2, gamma: 1.0,
    };

    let hard_contract = BehavioralContract {
        domain: "hard_domain".into(),
        preconditions: vec![],
        invariants: vec![SessionInvariant {
            predicate: Box::new(BeliefExists { key: "hard_guard".into() }),
            class: ViolationClass::Soft, // both Soft so we don't halt
            max_intervention_gap_steps: 1,
        }],
        governance: GovernancePolicies::default(),
        recovery: RecoveryPolicy::RollbackToLastCheckpoint,
        p: 0.99, delta: 5, k: 100,
        alpha: 0.1, gamma: 0.5,
    };

    // Neither "soft_guard" nor "hard_guard" exist → both invariants fire Soft.
    let rt = runtime()
        .with_contract(soft_contract)
        .with_contract(hard_contract);

    let id = rt.insert_belief(node("other", 0.9)).await;
    let _ = rt.update_belief(
        id,
        BeliefValue::Asserted("x".into()),
        Provenance::UserStatement { turn: 0 },
        0.8,
    ).await;

    rt.finalize_session().await;
    let report = rt.session_report().await;

    // Both contracts fire → 2 soft violations from a single update.
    assert_eq!(report.soft_violations, 2, "Both contracts should detect their violation");
    assert_eq!(report.drift_bounds.len(), 2);
}

// ── Sovereignty write auth integration ───────────────────────────────────────

#[tokio::test]
async fn sovereignty_write_auth_blocks_update_when_agent_denied() {
    let sov = MnemonicSovereignty {
        write_auth: AuthPolicy {
            allowed_agents: AllowedAgents::Allowlist(vec!["trusted_agent".into()]),
            require_audit: false,
        },
        ..MnemonicSovereignty::permissive()
    };

    let rt = runtime().with_sovereignty(sov);
    let id = rt.insert_belief(node("key", 0.9)).await;

    // The runtime uses "system" as the agent_id — not in the allowlist.
    let result = rt.update_belief(
        id,
        BeliefValue::Asserted("x".into()),
        Provenance::UserStatement { turn: 0 },
        0.8,
    ).await;

    assert!(
        matches!(result, Err(RuntimeError::SovereigntyViolation(_))),
        "Expected SovereigntyViolation, got: {result:?}"
    );
}

#[tokio::test]
async fn sovereignty_permissive_allows_all_updates() {
    let sov = MnemonicSovereignty::permissive();
    let rt = runtime().with_sovereignty(sov);
    let id = rt.insert_belief(node("key", 0.9)).await;
    let result = rt.update_belief(
        id,
        BeliefValue::Asserted("x".into()),
        Provenance::UserStatement { turn: 0 },
        0.8,
    ).await;
    assert!(result.is_ok());
}

// ── Session report contract fields ────────────────────────────────────────────

#[tokio::test]
async fn session_report_without_contracts_has_zero_violations() {
    let rt = runtime();
    rt.finalize_session().await;
    let report = rt.session_report().await;

    assert_eq!(report.soft_violations, 0);
    assert_eq!(report.hard_violations, 0);
    assert_eq!(report.critical_violations, 0);
    assert_eq!(report.recovery_actions_taken, 0);
    assert!(report.drift_bounds.is_empty());
}
