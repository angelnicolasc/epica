//! Unit tests for all nine MnemonicSovereignty primitives.
//!
//! Verifies every enforcement method added in Phase 3.

use epica_contracts::{
    AllowedAgents, AuditDestination, AuditEntry, AuditEventType, AuditMode,
    AuditPolicy, AuthPolicy, CrossAgentPolicy, ForgetPolicy, ForgetTrigger,
    ForgetVerificationResult, MnemonicSovereignty, RecoveryVerificationResult,
    RecoveryVerifier, RetentionPolicy, SharingMode, default_causal_verify_fn,
};
use epica_core::{BeliefNode, BeliefQuad, BeliefValue, CausalEdge, Provenance};

// ── Primitive (1)/(3)/(8): Authorization ─────────────────────────────────────

#[test]
fn write_auth_allowlist_blocks_unknown() {
    let mut sov = MnemonicSovereignty::permissive();
    sov.write_auth = AuthPolicy {
        allowed_agents: AllowedAgents::Allowlist(vec!["alice".into()]),
        require_audit: false,
    };
    assert!(sov.check_write_auth("alice").is_ok());
    assert!(sov.check_write_auth("bob").is_err());
}

#[test]
fn write_auth_denylist_blocks_listed() {
    let mut sov = MnemonicSovereignty::permissive();
    sov.write_auth = AuthPolicy {
        allowed_agents: AllowedAgents::Denylist(vec!["bob".into()]),
        require_audit: false,
    };
    assert!(sov.check_write_auth("alice").is_ok());
    assert!(sov.check_write_auth("bob").is_err());
}

#[test]
fn human_approval_required_blocks_all() {
    let mut sov = MnemonicSovereignty::permissive();
    sov.write_auth = AuthPolicy {
        allowed_agents: AllowedAgents::HumanApprovalRequired,
        require_audit: false,
    };
    assert!(sov.check_write_auth("alice").is_err());
    assert!(sov.write_auth.requires_human_approval());
}

#[test]
fn rollback_auth_enforced() {
    let mut sov = MnemonicSovereignty::permissive();
    sov.rollback_auth = AuthPolicy {
        allowed_agents: AllowedAgents::Allowlist(vec!["admin".into()]),
        require_audit: false,
    };
    assert!(sov.check_rollback_auth("admin").is_ok());
    assert!(sov.check_rollback_auth("user").is_err());
}

#[test]
fn permissive_allows_all_agents() {
    let sov = MnemonicSovereignty::permissive();
    assert!(sov.check_write_auth("anyone").is_ok());
    assert!(sov.check_update_auth("anyone").is_ok());
    assert!(sov.check_read_auth("anyone").is_ok());
    assert!(sov.check_rollback_auth("anyone").is_ok());
}

// ── Primitive (4): Retention ──────────────────────────────────────────────────

#[test]
fn retention_permanent_never_expires() {
    assert!(!RetentionPolicy::Permanent.is_expired(0, u64::MAX));
}

#[test]
fn retention_duration_expires_after_ttl() {
    assert!(RetentionPolicy::Duration(1000).is_expired(0, 2000));
    assert!(!RetentionPolicy::Duration(1000).is_expired(0, 500));
    // Boundary: exactly at TTL → not expired (exclusive >)
    assert!(!RetentionPolicy::Duration(1000).is_expired(0, 1000));
}

#[test]
fn retention_session_scoped_never_expires_via_is_expired() {
    // SessionScoped expiry is handled at session-end, not per-read.
    assert!(!RetentionPolicy::SessionScoped.is_expired(0, u64::MAX));
}

#[test]
fn sovereignty_is_belief_expired_delegates_to_retention() {
    let mut sov = MnemonicSovereignty::permissive();
    sov.retention = RetentionPolicy::Duration(500);
    assert!(sov.is_belief_expired(0, 1000));
    assert!(!sov.is_belief_expired(0, 300));
}

// ── Primitive (5): Forget ─────────────────────────────────────────────────────

#[test]
fn forget_removes_belief_and_verifies_when_no_descendants() {
    let mut quad = BeliefQuad::new();
    let id = quad.insert(BeliefNode::new(
        "secret",
        BeliefValue::Asserted("sensitive".into()),
        Provenance::UserStatement { turn: 0 },
        0.9,
    ));

    let policy = ForgetPolicy {
        triggers: vec![ForgetTrigger::ExplicitRequest {
            authorized_by: "admin".into(),
        }],
        verify_fn: Box::new(default_causal_verify_fn),
    };

    let result = policy.execute(id, &mut quad);
    // execute() verifies first (no causal descendants → Verified), then removes.
    assert!(matches!(result, ForgetVerificationResult::Verified));
    // The belief must be absent after execute().
    assert!(quad.get(id).is_none());
}

#[test]
fn forget_detects_causal_remnant() {
    let mut quad = BeliefQuad::new();
    let parent = quad.insert(BeliefNode::new(
        "parent",
        BeliefValue::Asserted("cause".into()),
        Provenance::UserStatement { turn: 0 },
        0.9,
    ));
    let child = quad.insert(BeliefNode::new(
        "child",
        BeliefValue::Asserted("effect".into()),
        Provenance::UserStatement { turn: 1 },
        0.8,
    ));
    quad.add_causal_edge(parent, child, CausalEdge::Causes { effect_size: 0.8, confidence: 0.9 });

    let policy = ForgetPolicy {
        triggers: vec![ForgetTrigger::TtlExpired],
        verify_fn: Box::new(default_causal_verify_fn),
    };

    // Remove the parent — the child is a causal descendant, so deletion is partial.
    let result = policy.execute(parent, &mut quad);
    assert!(matches!(result, ForgetVerificationResult::PartialDeletion { .. }));
}

#[test]
fn forget_is_triggered_by_matches_discriminant() {
    let policy = ForgetPolicy {
        triggers: vec![ForgetTrigger::TtlExpired, ForgetTrigger::ContractRevoked],
        verify_fn: Box::new(|_, _| ForgetVerificationResult::Verified),
    };
    assert!(policy.is_triggered_by(&ForgetTrigger::TtlExpired));
    assert!(policy.is_triggered_by(&ForgetTrigger::ContractRevoked));
    assert!(!policy.is_triggered_by(&ForgetTrigger::GovernanceMandated {
        policy_id: "x".into()
    }));
}

// ── Primitive (6): Audit ──────────────────────────────────────────────────────

#[test]
fn audit_entry_now_has_nonzero_timestamp() {
    let entry = AuditEntry::now(
        AuditEventType::BeliefWrite,
        Some("key".into()),
        Some("agent".into()),
        serde_json::json!({}),
    );
    assert!(entry.timestamp_ms > 0);
}

#[test]
fn audit_policy_emit_tracing_does_not_panic() {
    let policy = AuditPolicy {
        mode: AuditMode::Full,
        destination: AuditDestination::Tracing,
        format: Default::default(),
    };
    let entry = AuditEntry::now(
        AuditEventType::ContractViolation,
        Some("test_key".into()),
        None,
        serde_json::json!({ "reason": "test" }),
    );
    policy.emit(&entry); // must not panic
}

// ── Primitive (7): Cross-agent propagation ────────────────────────────────────

#[test]
fn cross_agent_isolated_blocks_all() {
    let p = CrossAgentPolicy {
        sharing_mode: SharingMode::Isolated,
        require_consent: false,
    };
    assert!(!p.allows_share_with("anyone"));
}

#[test]
fn cross_agent_allowlist_works() {
    let p = CrossAgentPolicy {
        sharing_mode: SharingMode::Allowlist(vec!["bob".into()]),
        require_consent: false,
    };
    assert!(p.allows_share_with("bob"));
    assert!(!p.allows_share_with("carol"));
}

#[test]
fn cross_agent_session_shared_allows_all() {
    let p = CrossAgentPolicy {
        sharing_mode: SharingMode::SessionShared,
        require_consent: false,
    };
    assert!(p.allows_share_with("any_agent"));
}

#[test]
fn sovereignty_allows_cross_agent_share_delegates() {
    let mut sov = MnemonicSovereignty::permissive();
    sov.cross_agent = CrossAgentPolicy {
        sharing_mode: SharingMode::Isolated,
        require_consent: false,
    };
    assert!(!sov.allows_cross_agent_share("anyone"));
}

// ── Primitive (9): Recovery verification ─────────────────────────────────────

#[test]
fn permissive_recovery_verify_always_passes() {
    let sov = MnemonicSovereignty::permissive();
    let quad = BeliefQuad::new();
    assert!(matches!(
        sov.verify_recovery(&quad),
        RecoveryVerificationResult::Verified
    ));
}

#[test]
fn custom_verify_fn_is_called_after_recovery() {
    use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

    let called = Arc::new(AtomicBool::new(false));
    let called_clone = called.clone();

    let mut sov = MnemonicSovereignty::permissive();
    sov.recovery_verify = RecoveryVerifier {
        verify_fn: Box::new(move |_| {
            called_clone.store(true, Ordering::SeqCst);
            RecoveryVerificationResult::Verified
        }),
    };

    let quad = BeliefQuad::new();
    let result = sov.verify_recovery(&quad);
    assert!(called.load(Ordering::SeqCst));
    assert!(matches!(result, RecoveryVerificationResult::Verified));
}

#[test]
fn custom_verify_fn_can_return_failed() {
    let sov = MnemonicSovereignty {
        write_auth: AuthPolicy::default(),
        read_auth: AuthPolicy::default(),
        update_auth: AuthPolicy::default(),
        retention: RetentionPolicy::Permanent,
        forget: None,
        audit: AuditPolicy::default(),
        cross_agent: CrossAgentPolicy::default(),
        rollback_auth: AuthPolicy::default(),
        recovery_verify: RecoveryVerifier {
            verify_fn: Box::new(|_| RecoveryVerificationResult::Failed {
                reason: "test failure".into(),
            }),
        },
    };
    let quad = BeliefQuad::new();
    assert!(matches!(
        sov.verify_recovery(&quad),
        RecoveryVerificationResult::Failed { .. }
    ));
}
