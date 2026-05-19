//! Mnemonic Sovereignty: nine memory governance primitives.
//!
//! From the Mnemonic Sovereignty survey (arXiv:2604.16548):
//! "No published architecture covers all nine governance primitives."
//! Epica is the first implementation to enforce all nine at mutation time.

pub mod audit;
pub mod auth;
pub mod cross_agent;
pub mod forget;
pub mod ledger;
pub mod retention;

pub use audit::{AuditDestination, AuditEntry, AuditEventType, AuditFormat, AuditMode, AuditPolicy};
pub use ledger::{
    new_shared_ledger, verify_merkle_proof, AuditLedger, LedgerEntry, LedgerTamperError,
    SharedLedger,
};
pub use auth::{AllowedAgents, AuthPolicy};
pub use cross_agent::{CrossAgentPolicy, SharingMode};
pub use forget::{default_causal_verify_fn, ForgetPolicy, ForgetTrigger, ForgetVerificationResult};
pub use retention::RetentionPolicy;

use crate::error::ContractError;

/// The complete memory governance layer — all nine sovereignty primitives.
///
/// Attach to a `BeliefRuntime` via `.with_sovereignty(sov)` to enforce
/// authorization, retention, audit, and right-to-forget at mutation time.
pub struct MnemonicSovereignty {
    /// (1) Write authorization — who may insert new beliefs.
    pub write_auth: AuthPolicy,
    /// (2) Read authorization — who may read beliefs.
    pub read_auth: AuthPolicy,
    /// (3) Update authorization — who may revise existing beliefs.
    pub update_auth: AuthPolicy,
    /// (4) Retention policy — how long each belief lives before expiry.
    pub retention: RetentionPolicy,
    /// (5) Forget policy — right to erasure with causal reachability verification.
    pub forget: Option<ForgetPolicy>,
    /// (6) Audit trail — structured events for every governance action.
    pub audit: AuditPolicy,
    /// (7) Cross-agent propagation — which agents may receive belief copies.
    pub cross_agent: CrossAgentPolicy,
    /// (8) Rollback authorization — who may revert belief state via checkpoint.
    pub rollback_auth: AuthPolicy,
    /// (9) Recovery verification — post-rollback correctness check.
    pub recovery_verify: RecoveryVerifier,
}

/// Post-rollback state verifier.
///
/// Supply a custom `verify_fn` to assert domain-specific post-conditions
/// after recovery (e.g. "the system_prompt belief must still be present").
pub struct RecoveryVerifier {
    pub verify_fn: Box<dyn Fn(&epica_core::BeliefQuad) -> RecoveryVerificationResult + Send + Sync>,
}

#[derive(Debug, Clone)]
pub enum RecoveryVerificationResult {
    Verified,
    Failed { reason: String },
}

// ── Constructors ──────────────────────────────────────────────────────────────

impl MnemonicSovereignty {
    /// Permissive defaults — suitable for development; tighten for production.
    pub fn permissive() -> Self {
        Self {
            write_auth: AuthPolicy::default(),
            read_auth: AuthPolicy::default(),
            update_auth: AuthPolicy::default(),
            retention: RetentionPolicy::Permanent,
            forget: None,
            audit: AuditPolicy::default(),
            cross_agent: CrossAgentPolicy::default(),
            rollback_auth: AuthPolicy::default(),
            recovery_verify: RecoveryVerifier {
                verify_fn: Box::new(|_| RecoveryVerificationResult::Verified),
            },
        }
    }
}

// ── Enforcement methods ───────────────────────────────────────────────────────

impl MnemonicSovereignty {
    /// Primitive (1): check that `agent_id` may write a new belief.
    pub fn check_write_auth(&self, agent_id: &str) -> Result<(), ContractError> {
        if self.write_auth.allows(agent_id) {
            Ok(())
        } else {
            Err(ContractError::SovereigntyViolation {
                message: format!("write_auth denied for agent '{agent_id}'"),
            })
        }
    }

    /// Primitive (3): check that `agent_id` may revise an existing belief.
    pub fn check_update_auth(&self, agent_id: &str) -> Result<(), ContractError> {
        if self.update_auth.allows(agent_id) {
            Ok(())
        } else {
            Err(ContractError::SovereigntyViolation {
                message: format!("update_auth denied for agent '{agent_id}'"),
            })
        }
    }

    /// Primitive (2): check that `agent_id` may read beliefs.
    pub fn check_read_auth(&self, agent_id: &str) -> Result<(), ContractError> {
        if self.read_auth.allows(agent_id) {
            Ok(())
        } else {
            Err(ContractError::SovereigntyViolation {
                message: format!("read_auth denied for agent '{agent_id}'"),
            })
        }
    }

    /// Primitive (8): check that `agent_id` may perform a rollback.
    pub fn check_rollback_auth(&self, agent_id: &str) -> Result<(), ContractError> {
        if self.rollback_auth.allows(agent_id) {
            Ok(())
        } else {
            Err(ContractError::SovereigntyViolation {
                message: format!("rollback_auth denied for agent '{agent_id}'"),
            })
        }
    }

    /// Primitive (4): whether a belief created at `created_at_ms` has expired.
    pub fn is_belief_expired(&self, created_at_ms: u64, now_ms: u64) -> bool {
        self.retention.is_expired(created_at_ms, now_ms)
    }

    /// Primitive (7): whether `agent_id` may receive a copy of this belief.
    pub fn allows_cross_agent_share(&self, agent_id: &str) -> bool {
        self.cross_agent.allows_share_with(agent_id)
    }

    /// Primitive (6): emit a structured audit entry.
    pub fn emit_audit(&self, entry: &AuditEntry) {
        self.audit.emit(entry);
    }

    /// Primitive (9): verify that the post-recovery quad state is correct.
    pub fn verify_recovery(&self, quad: &epica_core::BeliefQuad) -> RecoveryVerificationResult {
        (self.recovery_verify.verify_fn)(quad)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permissive_allows_all_agents() {
        let sov = MnemonicSovereignty::permissive();
        assert!(sov.check_write_auth("anyone").is_ok());
        assert!(sov.check_update_auth("anyone").is_ok());
        assert!(sov.check_read_auth("anyone").is_ok());
        assert!(sov.check_rollback_auth("anyone").is_ok());
    }

    #[test]
    fn write_auth_denylist_blocks() {
        let mut sov = MnemonicSovereignty::permissive();
        sov.write_auth = AuthPolicy {
            allowed_agents: AllowedAgents::Allowlist(vec!["alice".into()]),
            require_audit: false,
        };
        assert!(sov.check_write_auth("alice").is_ok());
        assert!(sov.check_write_auth("bob").is_err());
    }

    #[test]
    fn permissive_recovery_verify_passes() {
        let sov = MnemonicSovereignty::permissive();
        let quad = epica_core::BeliefQuad::new();
        assert!(matches!(sov.verify_recovery(&quad), RecoveryVerificationResult::Verified));
    }
}
