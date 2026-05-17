//! Recovery policies for contract violations.

use epica_core::CheckpointId;

/// What to do automatically when a session invariant is violated.
#[derive(Debug, Clone)]
pub enum RecoveryPolicy {
    /// Roll back to the most recent checkpoint.
    RollbackToLastCheckpoint,

    /// Roll back to a specific version.
    RollbackToVersion(u64),

    /// Emit the causal diff (root cause report) and halt.
    /// The diff is the analysis — no LLM-as-judge, no heuristics.
    EmitCausalDiffAndHalt,

    /// For high-availability systems: reduce confidence floors and continue.
    DegradeAndContinue { min_confidence: f32 },

    /// Roll back to a named checkpoint.
    RollbackToCheckpoint(CheckpointId),
}
