//! Python exception hierarchy for the Epica SDK.
//!
//! All exceptions inherit from `EpicaError` → `Exception`.
//!
//! ```text
//! Exception
//! └── EpicaError
//!     ├── BeliefRevisionError   — AGM postulate violations, belief not found
//!     ├── ContractViolationError — behavioral contract precondition / invariant failure
//!     ├── CheckpointError        — checkpoint not found, K*4 unnecessary contraction
//!     └── SovereigntyError       — mnemonic governance policy violation
//! ```

use pyo3::prelude::*;
use pyo3::PyTypeInfo;

pyo3::create_exception!(epica._epica, EpicaError,             pyo3::exceptions::PyException);
pyo3::create_exception!(epica._epica, BeliefRevisionError,    EpicaError);
pyo3::create_exception!(epica._epica, ContractViolationError, EpicaError);
pyo3::create_exception!(epica._epica, CheckpointError,        EpicaError);
pyo3::create_exception!(epica._epica, SovereigntyError,       EpicaError);

/// Register all exception types on the module.
pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    let py = m.py();
    m.add("EpicaError",             EpicaError::type_object_bound(py))?;
    m.add("BeliefRevisionError",    BeliefRevisionError::type_object_bound(py))?;
    m.add("ContractViolationError", ContractViolationError::type_object_bound(py))?;
    m.add("CheckpointError",        CheckpointError::type_object_bound(py))?;
    m.add("SovereigntyError",       SovereigntyError::type_object_bound(py))?;
    Ok(())
}

// ── Error mapping helpers ─────────────────────────────────────────────────────

pub(crate) fn map_revision_err(e: epica_core::BeliefRevisionError) -> PyErr {
    BeliefRevisionError::new_err(e.to_string())
}

pub(crate) fn map_rollback_err(e: epica_core::RollbackError) -> PyErr {
    CheckpointError::new_err(e.to_string())
}

pub(crate) fn map_runtime_err(e: epica_runtime::RuntimeError) -> PyErr {
    let msg = e.to_string();
    match e {
        epica_runtime::RuntimeError::ContractViolation { .. }
        | epica_runtime::RuntimeError::PreconditionFailed { .. }
        | epica_runtime::RuntimeError::ContractHalt { .. } => ContractViolationError::new_err(msg),
        epica_runtime::RuntimeError::SovereigntyViolation(_) => SovereigntyError::new_err(msg),
        epica_runtime::RuntimeError::GovernanceLimitExceeded { .. } => ContractViolationError::new_err(msg),
        _ => EpicaError::new_err(msg),
    }
}
