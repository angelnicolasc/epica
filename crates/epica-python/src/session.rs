//! `PySessionReport` — session-level metrics returned by `BeliefRuntime.finalize_session()`.

use pyo3::prelude::*;

use epica_runtime::SessionReport;

/// Summary of a completed `BeliefRuntime` session.
///
/// Obtain via `BeliefRuntime.finalize_session()`. All fields are read-only.
///
/// # Calibration target
/// A well-calibrated agent should achieve `trajectory_ece < 0.08` on sessions
/// with more than 20 belief revisions (BeliefShift benchmark, March 2026).
#[pyclass(frozen, name = "SessionReport")]
pub struct PySessionReport {
    /// Trajectory-ECE: `Σ_t |confidence_t − accuracy_t| / T`.
    ///
    /// `None` when no belief has a known outcome yet.
    #[pyo3(get)]
    pub trajectory_ece: Option<f32>,

    /// Total number of `update_belief()` calls in this session.
    #[pyo3(get)]
    pub total_revisions: usize,

    /// Number of revisions where AGM contraction fired.
    #[pyo3(get)]
    pub contradictions_detected: usize,

    /// Number of times System 2 (LLM reflection) was activated.
    #[pyo3(get)]
    pub system2_activations: u64,

    /// Number of times System 2 was skipped due to budget exhaustion.
    #[pyo3(get)]
    pub system2_throttled: u64,

    /// `True` when `trajectory_ece < 0.08` (or when no ECE is available yet).
    #[pyo3(get)]
    pub calibration_target_met: bool,

    /// Number of soft invariant violations logged this session.
    #[pyo3(get)]
    pub soft_violations: usize,

    /// Number of hard invariant violations that triggered recovery.
    #[pyo3(get)]
    pub hard_violations: usize,

    /// Number of critical invariant violations that halted execution.
    #[pyo3(get)]
    pub critical_violations: usize,

    /// Number of recovery actions taken (rollback, degrade, etc.).
    #[pyo3(get)]
    pub recovery_actions_taken: usize,

    /// Governance tokens consumed this session.
    #[pyo3(get)]
    pub governance_tokens_used: u64,
}

#[pymethods]
impl PySessionReport {
    fn __repr__(&self) -> String {
        let tece = self.trajectory_ece
            .map(|t| format!("{t:.4}"))
            .unwrap_or_else(|| "None".to_string());
        format!(
            "SessionReport(trajectory_ece={tece}, revisions={rev}, system2={s2}, calibrated={cal})",
            tece = tece,
            rev = self.total_revisions,
            s2 = self.system2_activations,
            cal = self.calibration_target_met,
        )
    }

    /// Return the report as a plain dict for JSON serialization.
    fn to_dict(&self, py: Python<'_>) -> PyResult<PyObject> {
        let dict = pyo3::types::PyDict::new_bound(py);
        dict.set_item("trajectory_ece", self.trajectory_ece)?;
        dict.set_item("total_revisions", self.total_revisions)?;
        dict.set_item("contradictions_detected", self.contradictions_detected)?;
        dict.set_item("system2_activations", self.system2_activations)?;
        dict.set_item("system2_throttled", self.system2_throttled)?;
        dict.set_item("calibration_target_met", self.calibration_target_met)?;
        dict.set_item("soft_violations", self.soft_violations)?;
        dict.set_item("hard_violations", self.hard_violations)?;
        dict.set_item("critical_violations", self.critical_violations)?;
        dict.set_item("recovery_actions_taken", self.recovery_actions_taken)?;
        dict.set_item("governance_tokens_used", self.governance_tokens_used)?;
        Ok(dict.into())
    }
}

pub(crate) fn report_to_py(r: SessionReport) -> PySessionReport {
    PySessionReport {
        trajectory_ece: r.tece,
        total_revisions: r.total_revisions,
        contradictions_detected: r.contradictions_detected,
        system2_activations: r.system2_activations,
        system2_throttled: r.system2_throttled,
        calibration_target_met: r.calibration_target_met,
        soft_violations: r.soft_violations,
        hard_violations: r.hard_violations,
        critical_violations: r.critical_violations,
        recovery_actions_taken: r.recovery_actions_taken,
        governance_tokens_used: r.governance_tokens_used,
    }
}
