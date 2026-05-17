//! Session-level metrics and summary report.

/// Calibration target from the BeliefShift benchmark (March 2026).
///
/// Sessions with more than 20 steps should achieve T-ECE < 0.08 under
/// normal operating conditions when System 2 is active and well-calibrated.
pub const TECE_TARGET: f32 = 0.08;

/// Summary of a completed `BeliefRuntime` session.
///
/// Obtain via [`crate::BeliefRuntime::session_report()`] after calling
/// [`crate::BeliefRuntime::finalize_session()`].
///
/// # Calibration target
/// A well-calibrated agent should achieve `tece < TECE_TARGET` (0.08) on
/// sessions with more than 20 belief revisions.
#[derive(Debug, Clone)]
pub struct SessionReport {
    /// Trajectory-ECE for this session: `Σ_t |confidence_t − accuracy_t| / T`.
    ///
    /// `None` if no belief has a known outcome yet (call `finalize_session()`
    /// before generating the report to ensure all surviving beliefs are counted).
    pub tece: Option<f32>,

    /// Total number of `update_belief()` calls in this session.
    pub total_revisions: usize,

    /// Number of revisions where AGM contraction fired (a belief was found to
    /// contradict new information and was retracted).
    pub contradictions_detected: usize,

    /// Number of times System 2 (LLM reflection) was successfully activated.
    pub system2_activations: u64,

    /// Number of times System 2 activation was skipped due to budget exhaustion.
    pub system2_throttled: u64,

    /// `true` when `tece < TECE_TARGET` (or when no tece is available yet).
    pub calibration_target_met: bool,

    // ── Phase 3: contract enforcement metrics ─────────────────────────────────

    /// Number of Soft invariant violations detected this session.
    ///
    /// ABC paper (arXiv:2602.22302) baseline: 5.2–6.8 per session.
    pub soft_violations: usize,

    /// Number of Hard invariant violations that triggered a recovery action.
    pub hard_violations: usize,

    /// Number of Critical invariant violations that halted execution.
    pub critical_violations: usize,

    /// Number of recovery actions executed (rollback, degrade, etc.).
    pub recovery_actions_taken: usize,

    /// Drift bounds D* = α/γ for each attached contract.
    pub drift_bounds: Vec<epica_contracts::DriftBound>,

    /// Governance tokens consumed this session.
    pub governance_tokens_used: u64,
}
