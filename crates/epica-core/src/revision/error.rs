use crate::{checkpoint::types::CheckpointId, diff::BeliefQuadDiff};

/// Errors that can occur during AGM belief revision.
#[derive(Debug, thiserror::Error)]
pub enum BeliefRevisionError {
    #[error("belief not found in quad")]
    BeliefNotFound,

    #[error("revision would produce an inconsistent belief set")]
    InconsistentRevision,

    #[error("postulate audit failed: {postulate}")]
    PostulateViolation { postulate: &'static str },
}

/// Errors that can occur during rollback.
#[derive(Debug, thiserror::Error)]
pub enum RollbackError {
    #[error("checkpoint {0:?} not found")]
    CheckpointNotFound(CheckpointId),

    /// K*4 violation: the contraction would remove beliefs that do not contradict
    /// the current state. The diff is returned for diagnosis.
    #[error("rollback would unnecessarily contract beliefs (K*4 violation)")]
    UnnecessaryContraction(BeliefQuadDiff),
}
