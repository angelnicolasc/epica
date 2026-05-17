/// Errors from behavioral contract operations.
#[derive(Debug, thiserror::Error)]
pub enum ContractError {
    #[error("precondition failed: {description}")]
    PreconditionFailed { description: String },

    #[error("invariant violation: {description} (class: {class:?})")]
    InvariantViolation {
        description: String,
        class: crate::invariant::ViolationClass,
    },

    #[error("sovereignty violation: {message}")]
    SovereigntyViolation { message: String },
}
