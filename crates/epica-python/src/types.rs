//! Python-facing data types: `PyBeliefNode`, `PyProvenance`.

use pyo3::prelude::*;

use epica_core::{BeliefNode, BeliefValue, Provenance};

/// A snapshot of a single belief node, returned from `BeliefQuad.get()`.
///
/// All fields are read-only after creation (`frozen` pyclass).
#[pyclass(frozen, name = "BeliefNode")]
pub struct PyBeliefNode {
    /// Domain-scoped key, e.g. `"user_intent"` or `"file_tree"`.
    #[pyo3(get)]
    pub key: String,

    /// Human-readable representation of the belief value.
    #[pyo3(get)]
    pub value: String,

    /// Value kind: `"asserted"`, `"inferred"`, `"deterministic"`, or `"reference"`.
    #[pyo3(get)]
    pub value_kind: String,

    /// Provenance kind: `"user_statement"`, `"llm_inference"`, `"tool_result"`,
    /// or `"runtime_inference"`.
    #[pyo3(get)]
    pub provenance: String,

    /// System 1 (fast) confidence — always in `[0.0, 1.0]`.
    #[pyo3(get)]
    pub fast_confidence: f32,

    /// System 2 (slow) confidence — `None` until System 2 has reflected on this belief.
    #[pyo3(get)]
    pub slow_confidence: Option<f32>,

    /// Creation timestamp as milliseconds since UNIX epoch.
    #[pyo3(get)]
    pub created_at_ms: u64,

    /// Effective confidence: `slow_confidence` when available, else `fast_confidence`.
    #[pyo3(get)]
    pub effective_confidence: f32,
}

#[pymethods]
impl PyBeliefNode {
    fn __repr__(&self) -> String {
        format!(
            "BeliefNode(key={:?}, value_kind={:?}, fast_confidence={:.3}, effective_confidence={:.3})",
            self.key, self.value_kind, self.fast_confidence, self.effective_confidence
        )
    }

    fn __str__(&self) -> String {
        format!("{} ({}) @ {:.3}", self.key, self.value_kind, self.effective_confidence)
    }

    fn __eq__(&self, other: &Self) -> bool {
        self.key == other.key && self.value == other.value
    }
}

/// Convert a `BeliefNode` reference to `PyBeliefNode`.
pub(crate) fn node_to_py(node: &BeliefNode) -> PyBeliefNode {
    let (value_str, value_kind) = match &node.value {
        BeliefValue::Asserted(s) => (s.clone(), "asserted".to_string()),
        BeliefValue::Inferred(v) => (v.to_string(), "inferred".to_string()),
        BeliefValue::Deterministic(v) => (v.to_string(), "deterministic".to_string()),
        BeliefValue::Reference(id) => (format!("ref:{id:?}"), "reference".to_string()),
    };
    let provenance = match &node.provenance {
        Provenance::UserStatement { .. } => "user_statement".to_string(),
        Provenance::LlmInference { model, .. } => format!("llm_inference:{model}"),
        Provenance::ToolResult { tool, .. } => format!("tool_result:{tool}"),
        Provenance::RuntimeInference { .. } => "runtime_inference".to_string(),
    };
    let effective = node.slow_confidence.unwrap_or(node.fast_confidence);
    PyBeliefNode {
        key: node.key.clone(),
        value: value_str,
        value_kind,
        provenance,
        fast_confidence: node.fast_confidence,
        slow_confidence: node.slow_confidence,
        created_at_ms: node.created_at_ms,
        effective_confidence: effective,
    }
}
