use epica_core::EpicaError;

/// Result of a `BeliefRuntime::update_belief()` call.
#[derive(Debug)]
pub enum RuntimeUpdateResult {
    /// System 1 ran; System 2 was not triggered.
    System1Only,

    /// System 2 was activated and completed synchronously (legacy; not used in server mode).
    System2Activated {
        diagnostic: crate::system2::DiagnosticSignal,
        result: crate::system2::System2Result,
    },

    /// System 2 eligibility confirmed; budget NOT yet consumed.
    ///
    /// The caller (MCP handler) must:
    /// 1. Consume one budget token via `runtime.try_consume_system2_budget()`.
    /// 2. Spawn an async task: call LLM, then `runtime.apply_system2_result()`.
    /// 3. Refund via `runtime.release_system2_budget()` if the LLM call fails.
    #[cfg(feature = "system2")]
    System2Pending {
        signal: crate::system2::DiagnosticSignal,
    },

    /// System 2 was triggered but the budget was exhausted.
    System2Throttled,
}

/// Errors from `BeliefRuntime` operations.
#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("core error: {0}")]
    Core(#[from] EpicaError),

    #[error("llm client error: {0}")]
    LlmClient(#[from] crate::system2::LlmClientError),

    #[error("contract violation: {message}")]
    ContractViolation { message: String },

    // ── Phase 3: contract enforcement errors ──────────────────────────────────

    /// A precondition check failed — the belief update was rejected.
    #[error("precondition failed in contract '{domain}' for belief '{belief_key}'")]
    PreconditionFailed { domain: String, belief_key: String },

    /// A Critical invariant was violated — the runtime halted.
    #[error("contract halt in '{domain}': invariant '{invariant}' violated at version {at_version}")]
    ContractHalt { domain: String, invariant: String, at_version: u64 },

    /// A sovereignty auth check blocked the operation.
    #[error("sovereignty violation: {0}")]
    SovereigntyViolation(String),

    /// A governance resource limit was exceeded (tokens, tool calls, etc.).
    #[error("governance limit exceeded: {resource} used {used}, limit {limit}")]
    GovernanceLimitExceeded { resource: String, limit: u64, used: u64 },

    /// A recovery action failed (e.g. checkpoint not found).
    #[error("recovery failed: {0}")]
    RecoveryFailed(String),
}
