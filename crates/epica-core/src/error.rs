use crate::revision::error::{BeliefRevisionError, RollbackError};

/// Top-level error type for `epica-core`.
#[derive(Debug, thiserror::Error)]
pub enum EpicaError {
    #[error("belief revision failed: {0}")]
    Revision(#[from] BeliefRevisionError),

    #[error("rollback failed: {0}")]
    Rollback(#[from] RollbackError),

    #[error("belief not found")]
    NotFound,

    #[error("invariant violated: {message}")]
    InvariantViolation { message: String },
}
