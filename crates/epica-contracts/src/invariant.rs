//! Session invariants and violation classes.
//!
//! Phase 3 implementation.

use crate::predicate::BeliefPredicate;

/// Severity class of a contract invariant violation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViolationClass {
    /// Log the violation; do not interrupt execution.
    Soft,

    /// Activate the recovery policy immediately.
    Hard,

    /// Halt execution, emit causal diff, escalate to human-in-the-loop.
    Critical,
}

/// A session invariant: a predicate that must hold at every `update_belief()` call.
pub struct SessionInvariant {
    pub predicate: Box<dyn BeliefPredicate>,
    pub class: ViolationClass,
    /// Maximum steps between violation detection and intervention before damage is irreversible.
    /// From StepShield (2026).
    pub max_intervention_gap_steps: u32,
}

/// A recorded violation of a session invariant.
#[derive(Debug, Clone)]
pub struct ContractViolation {
    pub violation_id: uuid::Uuid,
    pub invariant_description: String,
    pub class: ViolationClass,
    pub at_version: u64,
    /// The most recent checkpoint before the violation, if any exists.
    pub last_valid_checkpoint: Option<epica_core::CheckpointId>,
}
