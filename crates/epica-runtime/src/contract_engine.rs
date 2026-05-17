//! Contract enforcement engine for `BeliefRuntime`.
//!
//! `ContractEngine` bridges `epica-contracts` and the runtime update loop.
//! It adds two hooks around every `update_belief()` call:
//!
//! **Pre-update** (before AGM revision):
//!   1. Sovereignty write-auth check.
//!   2. Precondition evaluation for each `BehavioralContract`.
//!
//! **Post-update** (after AGM revision + System 1):
//!   1. Collect ALL invariant violations per contract (Soft, Hard, Critical).
//!   2. Log Soft violations (non-blocking).
//!   3. Execute the recovery policy on the first Hard violation.
//!   4. Halt on the first Critical violation → `RuntimeError::ContractHalt`.
//!   5. Emit a structured audit entry via `MnemonicSovereignty`.
//!
//! This is what distinguishes Epica from output-level guardrails: enforcement
//! at mutation time, not session-end.

use std::sync::{Arc, Mutex};

use epica_contracts::{
    BehavioralContract, ContractViolation, MnemonicSovereignty, RecoveryPolicy,
    ViolationClass, AuditEntry, AuditEventType, RecoveryVerificationResult,
};
use epica_core::{BeliefId, BeliefQuad, BeliefQuadDiff, CheckpointId};

use crate::error::RuntimeError;

// ── Violation log ─────────────────────────────────────────────────────────────

/// Per-session violation accumulator shared across all update calls.
#[derive(Default)]
pub struct ViolationLog {
    pub soft: Vec<ContractViolation>,
    pub hard: Vec<ContractViolation>,
    pub critical: Vec<ContractViolation>,
    pub recovery_actions_taken: usize,
}

// ── RecoveryOutcome ───────────────────────────────────────────────────────────

/// Returned when a Hard violation triggered a recovery action.
pub struct RecoveryOutcome {
    pub violation: ContractViolation,
    pub diff: Option<BeliefQuadDiff>,
    pub recovery_applied: RecoveryPolicy,
    pub verification: RecoveryVerificationResult,
}

// ── ContractEngine ────────────────────────────────────────────────────────────

/// Orchestrates behavioral contract enforcement inside `BeliefRuntime`.
pub struct ContractEngine {
    pub(crate) contracts: Vec<BehavioralContract>,
    pub(crate) sovereignty: Option<MnemonicSovereignty>,
    pub(crate) violation_log: Arc<Mutex<ViolationLog>>,
}

impl Default for ContractEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl ContractEngine {
    pub fn new() -> Self {
        Self {
            contracts: Vec::new(),
            sovereignty: None,
            violation_log: Arc::new(Mutex::new(ViolationLog::default())),
        }
    }

    // ── Pre-update ────────────────────────────────────────────────────────────

    /// Run before AGM revision: sovereignty auth + contract preconditions.
    ///
    /// Returns `Err` if any check fails — the caller must abort the update.
    pub fn pre_update(
        &self,
        quad: &BeliefQuad,
        belief_key: &str,
        agent_id: &str,
    ) -> Result<(), RuntimeError> {
        if let Some(sov) = &self.sovereignty {
            sov.check_write_auth(agent_id)
                .map_err(|e| RuntimeError::SovereigntyViolation(e.to_string()))?;
        }

        for contract in &self.contracts {
            if !contract.check_preconditions(quad) {
                return Err(RuntimeError::PreconditionFailed {
                    domain: contract.domain.clone(),
                    belief_key: belief_key.to_string(),
                });
            }
        }

        Ok(())
    }

    // ── Post-update ───────────────────────────────────────────────────────────

    /// Run after AGM revision + System 1.
    ///
    /// Collects all violations, executes recovery for Hard, halts for Critical,
    /// and emits an audit entry.
    pub fn post_update(
        &self,
        quad: &mut BeliefQuad,
        last_checkpoint: Option<CheckpointId>,
        belief_key: &str,
        agent_id: &str,
    ) -> Result<Option<RecoveryOutcome>, RuntimeError> {
        let mut outcome: Option<RecoveryOutcome> = None;

        for contract in &self.contracts {
            let violations = contract.collect_all_violations(quad, last_checkpoint);

            let mut saw_hard = false;

            for violation in violations {
                match violation.class {
                    ViolationClass::Soft => {
                        tracing::warn!(
                            domain = %contract.domain,
                            invariant = %violation.invariant_description,
                            version = violation.at_version,
                            "[epica] soft violation detected"
                        );
                        self.violation_log.lock().unwrap().soft.push(violation);
                    }

                    ViolationClass::Hard => {
                        if !saw_hard && outcome.is_none() {
                            tracing::error!(
                                domain = %contract.domain,
                                invariant = %violation.invariant_description,
                                version = violation.at_version,
                                "[epica] hard violation — executing recovery"
                            );
                            let (diff, applied) =
                                execute_recovery(quad, &violation, &contract.recovery)?;

                            let verification = self
                                .sovereignty
                                .as_ref()
                                .map(|s| s.verify_recovery(quad))
                                .unwrap_or(RecoveryVerificationResult::Verified);

                            {
                                let mut log = self.violation_log.lock().unwrap();
                                log.hard.push(violation.clone());
                                log.recovery_actions_taken += 1;
                            }

                            outcome = Some(RecoveryOutcome {
                                violation,
                                diff,
                                recovery_applied: applied,
                                verification,
                            });
                            saw_hard = true;
                        } else {
                            // Additional Hard violations after the first — log only.
                            self.violation_log.lock().unwrap().hard.push(violation);
                        }
                    }

                    ViolationClass::Critical => {
                        tracing::error!(
                            domain = %contract.domain,
                            invariant = %violation.invariant_description,
                            version = violation.at_version,
                            "[epica] CRITICAL violation — halting"
                        );
                        self.violation_log.lock().unwrap().critical.push(violation.clone());
                        return Err(RuntimeError::ContractHalt {
                            domain: contract.domain.clone(),
                            invariant: violation.invariant_description,
                            at_version: violation.at_version,
                        });
                    }
                }
            }
        }

        // Primitive (6): always emit audit entry if sovereignty is configured.
        if let Some(sov) = &self.sovereignty {
            let entry = AuditEntry::now(
                AuditEventType::BeliefUpdate,
                Some(belief_key.to_string()),
                Some(agent_id.to_string()),
                serde_json::json!({
                    "recovery_triggered": outcome.is_some(),
                }),
            );
            sov.emit_audit(&entry);
        }

        Ok(outcome)
    }

    // ── Accessors ─────────────────────────────────────────────────────────────

    /// Returns `(soft, hard, critical, recovery_actions)` counts for this session.
    pub fn violation_counts(&self) -> (usize, usize, usize, usize) {
        let log = self.violation_log.lock().unwrap();
        (log.soft.len(), log.hard.len(), log.critical.len(), log.recovery_actions_taken)
    }

    /// Drift bounds D* = α/γ for each attached contract.
    pub fn drift_bounds(&self) -> Vec<epica_contracts::DriftBound> {
        self.contracts.iter().map(|c| c.drift_bound()).collect()
    }
}

// ── Recovery dispatch (free fn to avoid borrow issues) ───────────────────────

/// Execute a `RecoveryPolicy` against the quad.
///
/// Kept as a free function rather than a method so the borrow checker can see
/// that `quad` is mutably borrowed only in the match arms that need it.
fn execute_recovery(
    quad: &mut BeliefQuad,
    violation: &ContractViolation,
    policy: &RecoveryPolicy,
) -> Result<(Option<BeliefQuadDiff>, RecoveryPolicy), RuntimeError> {
    match policy {
        RecoveryPolicy::RollbackToLastCheckpoint => {
            let cp = violation.last_valid_checkpoint.ok_or_else(|| {
                RuntimeError::RecoveryFailed(
                    "RollbackToLastCheckpoint: no checkpoint available".into(),
                )
            })?;
            let diff = quad
                .rollback_to(cp)
                .map_err(|e| RuntimeError::RecoveryFailed(format!("rollback: {e:?}")))?;
            Ok((Some(diff), RecoveryPolicy::RollbackToLastCheckpoint))
        }

        RecoveryPolicy::RollbackToCheckpoint(cp) => {
            let diff = quad
                .rollback_to(*cp)
                .map_err(|e| RuntimeError::RecoveryFailed(format!("rollback: {e:?}")))?;
            Ok((Some(diff), RecoveryPolicy::RollbackToCheckpoint(*cp)))
        }

        RecoveryPolicy::RollbackToVersion(target_version) => {
            // Collect ID before the mutable borrow for rollback.
            let cp_id = quad
                .checkpoints()
                .iter()
                .filter(|c| c.version <= *target_version)
                .max_by_key(|c| c.version)
                .map(|c| c.id)
                .ok_or_else(|| {
                    RuntimeError::RecoveryFailed(format!(
                        "no checkpoint at or before version {target_version}"
                    ))
                })?;
            let diff = quad
                .rollback_to(cp_id)
                .map_err(|e| RuntimeError::RecoveryFailed(format!("rollback: {e:?}")))?;
            Ok((Some(diff), RecoveryPolicy::RollbackToVersion(*target_version)))
        }

        RecoveryPolicy::DegradeAndContinue { min_confidence } => {
            let floor = *min_confidence;
            // Collect IDs first; get_mut requires the borrow to end.
            let ids: Vec<BeliefId> = quad.iter().map(|(id, _)| id).collect();
            for id in ids {
                if let Some(node) = quad.get_mut(id) {
                    if node.fast_confidence < floor {
                        node.fast_confidence = floor;
                    }
                }
            }
            Ok((None, RecoveryPolicy::DegradeAndContinue { min_confidence: floor }))
        }

        RecoveryPolicy::EmitCausalDiffAndHalt => {
            // Clone the checkpoint quad to avoid a borrow conflict with quad.diff().
            let snap = violation.last_valid_checkpoint.and_then(|cp| {
                quad.checkpoints()
                    .iter()
                    .find(|c| c.id == cp)
                    .map(|c| c.quad.clone())
            });
            let diff = snap.map(|s| quad.diff(&s));

            if let Some(ref d) = diff {
                tracing::error!(diff = ?d, "[epica] causal diff (root cause report)");
            }

            Err(RuntimeError::ContractHalt {
                domain: "EmitCausalDiffAndHalt".into(),
                invariant: violation.invariant_description.clone(),
                at_version: violation.at_version,
            })
        }
    }
}
