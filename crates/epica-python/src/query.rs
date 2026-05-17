//! `PyBeliefDiff` and `PyCounterfactualResult` — diff and counterfactual outputs.

use pyo3::prelude::*;

use epica_core::BeliefQuadDiff;

/// Structural diff between two `BeliefQuad` states.
///
/// Returned by `BeliefQuad.rollback_to()` and `BeliefQuad.diff_with_checkpoint()`.
/// All fields are read-only (`frozen` pyclass).
#[pyclass(frozen, name = "BeliefDiff")]
pub struct PyBeliefDiff {
    /// Keys of beliefs added since the checkpoint.
    #[pyo3(get)]
    pub added: Vec<String>,

    /// Keys of beliefs removed since the checkpoint.
    #[pyo3(get)]
    pub removed: Vec<String>,

    /// Keys of beliefs whose value or confidence changed since the checkpoint.
    #[pyo3(get)]
    pub modified: Vec<String>,

    /// `True` when any of `added`, `modified`, or `removed` are non-empty.
    /// Represents the K*4 "is this rollback necessary" check.
    #[pyo3(get)]
    pub has_contradictions: bool,

    /// Trajectory-ECE for this diff interval, if available.
    #[pyo3(get)]
    pub trajectory_ece: Option<f32>,
}

#[pymethods]
impl PyBeliefDiff {
    /// Total number of changed beliefs (added + removed + modified).
    fn __len__(&self) -> usize {
        self.added.len() + self.removed.len() + self.modified.len()
    }

    fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.modified.is_empty()
    }

    fn __repr__(&self) -> String {
        format!(
            "BeliefDiff(+{added} -{removed} ~{modified}, contradictions={contradictions})",
            added = self.added.len(),
            removed = self.removed.len(),
            modified = self.modified.len(),
            contradictions = self.has_contradictions,
        )
    }

    /// Return the diff as a plain dict for JSON serialization.
    fn to_dict(&self, py: Python<'_>) -> PyResult<PyObject> {
        let dict = pyo3::types::PyDict::new_bound(py);
        dict.set_item("added", &self.added)?;
        dict.set_item("removed", &self.removed)?;
        dict.set_item("modified", &self.modified)?;
        dict.set_item("has_contradictions", self.has_contradictions)?;
        dict.set_item("trajectory_ece", self.trajectory_ece)?;
        Ok(dict.into())
    }
}

pub(crate) fn diff_to_py(diff: BeliefQuadDiff) -> PyBeliefDiff {
    let added: Vec<String> = diff.added.into_iter().map(|s| s.key).collect();
    let removed: Vec<String> = diff.removed.into_iter().map(|s| s.key).collect();
    let modified: Vec<String> = diff.modified.into_iter().map(|m| m.key).collect();
    let has = !added.is_empty() || !removed.is_empty() || !modified.is_empty();
    PyBeliefDiff {
        has_contradictions: has,
        trajectory_ece: diff.trajectory_ece,
        added,
        removed,
        modified,
    }
}

/// The surviving beliefs after a counterfactual removal.
///
/// Returned by `BeliefQuad.counterfactual(key)`.
#[pyclass(frozen, name = "CounterfactualResult")]
pub struct PyCounterfactualResult {
    /// The key of the removed antecedent belief.
    #[pyo3(get)]
    pub removed_key: String,

    /// Keys of all beliefs that survive in this counterfactual world.
    #[pyo3(get)]
    pub surviving: Vec<String>,

    /// Number of beliefs excluded (antecedent + its causal descendants).
    #[pyo3(get)]
    pub excluded_count: usize,
}

#[pymethods]
impl PyCounterfactualResult {
    fn __repr__(&self) -> String {
        format!(
            "CounterfactualResult(removed_key={:?}, surviving={}, excluded={})",
            self.removed_key,
            self.surviving.len(),
            self.excluded_count,
        )
    }

    fn __len__(&self) -> usize {
        self.surviving.len()
    }
}
