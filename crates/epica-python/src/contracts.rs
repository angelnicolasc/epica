//! `PyBehavioralContract` — the C=(P,I,G,R) formalism exposed to Python.
//!
//! From Agent Behavioral Contracts (arXiv:2602.22302).

use std::sync::Mutex;

use pyo3::prelude::*;

use epica_contracts::{
    BehavioralContract, GovernancePolicies, RecoveryPolicy, SessionInvariant, ViolationClass,
    predicate::{BeliefExists, MinConfidence},
};

use crate::belief::PyBeliefQuad;
use crate::error::ContractViolationError;

/// A typed behavioral contract: `C = (P, I, G, R)`.
///
/// - **P** (preconditions): must hold in the `BeliefQuad` before any action.
/// - **I** (invariants): must hold at every `update_belief()` call.
/// - **G** (governance): resource limits and authorization gates.
/// - **R** (recovery): action taken when an invariant is violated.
///
/// From arXiv:2602.22302 — enforces (p, δ, k)-satisfaction:
/// probability `p` that no more than `δ` violations occur in `k` steps.
///
/// ## Example
/// ```python
/// from epica import BeliefQuad, BehavioralContract
///
/// contract = BehavioralContract("deployment_safety")
/// contract.add_precondition("environment", min_confidence=0.8)
/// contract.add_invariant("user_goal", min_confidence=0.5)
///
/// quad = BeliefQuad()
/// quad.insert("environment", "staging", 0.9)
///
/// assert contract.check_preconditions(quad)  # passes
///
/// empty = BeliefQuad()
/// try:
///     contract.check_preconditions(empty)  # raises
/// except ContractViolationError as e:
///     print(e)
/// ```
#[pyclass(name = "BehavioralContract")]
pub struct PyBehavioralContract {
    inner: Mutex<BehavioralContract>,
}

#[pymethods]
impl PyBehavioralContract {
    // ── Constructor ───────────────────────────────────────────────────────────

    /// Create a new behavioral contract with the given domain name.
    ///
    /// `alpha` is the natural drift rate; `gamma` is the enforcement rate.
    /// The drift bound D* = α/γ is the expected maximum drift (ABC paper §3.2).
    #[new]
    #[pyo3(signature = (domain, alpha = 0.05, gamma = 0.5))]
    pub fn new(domain: &str, alpha: f64, gamma: f64) -> Self {
        let contract = BehavioralContract {
            domain: domain.to_string(),
            preconditions: vec![],
            invariants: vec![],
            governance: GovernancePolicies::default(),
            recovery: RecoveryPolicy::RollbackToLastCheckpoint,
            p: 0.99,
            delta: 1,
            k: 100,
            alpha,
            gamma,
        };
        Self { inner: Mutex::new(contract) }
    }

    // ── Preconditions ─────────────────────────────────────────────────────────

    /// Add a precondition: belief `key` must exist with `fast_confidence >= min_confidence`.
    #[pyo3(signature = (key, min_confidence = 0.5))]
    pub fn add_precondition(&self, key: &str, min_confidence: f32) {
        self.inner.lock().unwrap().preconditions.push(Box::new(MinConfidence {
            key: key.to_string(),
            threshold: min_confidence,
        }));
    }

    /// Add a precondition: belief `key` must exist (any confidence).
    pub fn add_presence_precondition(&self, key: &str) {
        self.inner.lock().unwrap().preconditions.push(Box::new(BeliefExists {
            key: key.to_string(),
        }));
    }

    // ── Invariants ────────────────────────────────────────────────────────────

    /// Add a Hard invariant: `key` must have `fast_confidence >= min_confidence` at all times.
    ///
    /// `severity` is one of `"soft"` (default), `"hard"`, or `"critical"`.
    #[pyo3(signature = (key, min_confidence = 0.5, severity = "hard"))]
    pub fn add_invariant(&self, key: &str, min_confidence: f32, severity: &str) -> PyResult<()> {
        let class = match severity {
            "soft"     => ViolationClass::Soft,
            "hard"     => ViolationClass::Hard,
            "critical" => ViolationClass::Critical,
            other => return Err(pyo3::exceptions::PyValueError::new_err(
                format!("unknown severity: {other:?}; use 'soft', 'hard', or 'critical'")
            )),
        };
        self.inner.lock().unwrap().invariants.push(SessionInvariant {
            predicate: Box::new(MinConfidence {
                key: key.to_string(),
                threshold: min_confidence,
            }),
            class,
            max_intervention_gap_steps: 5,
        });
        Ok(())
    }

    // ── Governance ────────────────────────────────────────────────────────────

    /// Set a maximum token budget for this contract's sessions.
    pub fn set_max_tokens(&self, limit: u64) {
        self.inner.lock().unwrap().governance.max_tokens_per_session = Some(limit);
    }

    // ── Evaluation ────────────────────────────────────────────────────────────

    /// Return `True` if all preconditions hold against the given `BeliefQuad`.
    ///
    /// Use `check_preconditions_raising()` to get an error on failure instead.
    pub fn check_preconditions(&self, quad: &PyBeliefQuad) -> bool {
        let contract = self.inner.lock().unwrap();
        let state = quad.state.read().unwrap();
        contract.check_preconditions(&state.quad)
    }

    /// Assert that all preconditions hold. Raises `ContractViolationError` on failure.
    pub fn check_preconditions_raising(&self, quad: &PyBeliefQuad) -> PyResult<()> {
        if self.check_preconditions(quad) {
            Ok(())
        } else {
            let domain = self.inner.lock().unwrap().domain.clone();
            Err(ContractViolationError::new_err(
                format!("precondition failed in contract '{domain}'")
            ))
        }
    }

    /// Return `True` if all Hard/Critical invariants hold against the given `BeliefQuad`.
    pub fn check_invariants(&self, quad: &PyBeliefQuad) -> bool {
        let contract = self.inner.lock().unwrap();
        let state = quad.state.read().unwrap();
        contract.check_invariants(&state.quad, None).is_none()
    }

    /// Assert that all Hard/Critical invariants hold. Raises `ContractViolationError` on failure.
    pub fn check_invariants_raising(&self, quad: &PyBeliefQuad) -> PyResult<()> {
        let contract = self.inner.lock().unwrap();
        let state = quad.state.read().unwrap();
        if let Some(violation) = contract.check_invariants(&state.quad, None) {
            Err(ContractViolationError::new_err(format!(
                "invariant violated in contract '{}': {} (class={:?})",
                contract.domain, violation.invariant_description, violation.class,
            )))
        } else {
            Ok(())
        }
    }

    /// Compute the drift bound D* = α/γ for this contract.
    pub fn drift_bound(&self) -> f64 {
        self.inner.lock().unwrap().drift_bound().expected_drift
    }

    // ── Metadata ──────────────────────────────────────────────────────────────

    #[getter]
    pub fn domain(&self) -> String {
        self.inner.lock().unwrap().domain.clone()
    }

    #[getter]
    pub fn precondition_count(&self) -> usize {
        self.inner.lock().unwrap().preconditions.len()
    }

    #[getter]
    pub fn invariant_count(&self) -> usize {
        self.inner.lock().unwrap().invariants.len()
    }

    fn __repr__(&self) -> String {
        let c = self.inner.lock().unwrap();
        format!(
            "BehavioralContract(domain={:?}, preconditions={}, invariants={}, drift_bound={:.3})",
            c.domain,
            c.preconditions.len(),
            c.invariants.len(),
            c.alpha / c.gamma,
        )
    }
}
