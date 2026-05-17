//! `PyBeliefQuad` — the full four-graph belief store, exposed to Python.
//!
//! Thread-safe: internal state is wrapped in `std::sync::RwLock` so that
//! GIL-free Python 3.13+ callers can share a quad across threads.

use std::collections::HashMap;
use std::sync::RwLock;

use pyo3::prelude::*;

use epica_core::{
    BeliefId, BeliefNode, BeliefQuad, BeliefValue, CheckpointId, Provenance,
    quad::{CausalEdge, SemanticEdge, TemporalEdge},
};

use crate::error::{map_revision_err, map_rollback_err, CheckpointError, BeliefRevisionError};
use crate::query::{diff_to_py, PyBeliefDiff, PyCounterfactualResult};
use crate::types::{node_to_py, PyBeliefNode};

// ── Internal state ────────────────────────────────────────────────────────────

pub(crate) struct QuadState {
    pub(crate) quad: BeliefQuad,
    /// O(1) key → BeliefId index kept in sync with the quad.
    pub(crate) key_to_id: HashMap<String, BeliefId>,
}

impl QuadState {
    fn new() -> Self {
        Self { quad: BeliefQuad::new(), key_to_id: HashMap::new() }
    }

    fn rebuild_index(&mut self) {
        self.key_to_id = self.quad.iter().map(|(id, n)| (n.key.clone(), id)).collect();
    }
}

// ── PyBeliefQuad ──────────────────────────────────────────────────────────────

/// A four-graph belief store implementing formal AGM belief revision.
///
/// Stores beliefs simultaneously in four orthogonal graphs (semantic, temporal,
/// causal, entity) over a shared node set. All mutations satisfy the AGM
/// postulates K*2–K*6 (Hansson 1999; MAGMA arXiv:2601.03236).
///
/// ## Thread safety
/// Internally wrapped in `RwLock` — safe for use from GIL-free Python 3.13+
/// threads without additional synchronisation.
///
/// ## Example
/// ```python
/// from epica import BeliefQuad
///
/// quad = BeliefQuad()
/// quad.insert("user_intent", "deploy to staging", 0.95)
/// quad.insert("environment", "production", 0.80)
/// quad.add_causal_edge("environment", "user_intent")
///
/// cp = quad.checkpoint()
/// quad.revise("user_intent", "deploy to production", 0.70)
///
/// diff = quad.rollback_to(cp)
/// print(diff)  # BeliefDiff(+0 -0 ~1, contradictions=True)
/// ```
#[pyclass(name = "BeliefQuad")]
pub struct PyBeliefQuad {
    pub(crate) state: RwLock<QuadState>,
}

#[pymethods]
impl PyBeliefQuad {
    // ── Constructor ───────────────────────────────────────────────────────────

    #[new]
    pub fn new() -> Self {
        Self { state: RwLock::new(QuadState::new()) }
    }

    // ── CRUD ──────────────────────────────────────────────────────────────────

    /// Insert a new belief. Returns `None` (keys are the stable identifiers).
    ///
    /// `provenance` is one of `"user"` (default), `"llm"`, or `"tool"`.
    /// `llm_model` names the LLM when `provenance="llm"` (e.g. `"claude-opus-4-7"`).
    /// `tool_name` names the tool when `provenance="tool"` (e.g. `"bash"`).
    #[pyo3(signature = (key, value, confidence, provenance = None, llm_model = None, tool_name = None))]
    pub fn insert(
        &self,
        key: &str,
        value: &str,
        confidence: f32,
        provenance: Option<&str>,
        llm_model: Option<&str>,
        tool_name: Option<&str>,
    ) {
        let prov = parse_provenance(provenance, llm_model, tool_name);
        let node = BeliefNode::new(key, BeliefValue::Asserted(value.to_string()), prov, confidence);
        let mut state = self.state.write().unwrap();
        let id = state.quad.insert(node);
        state.key_to_id.insert(key.to_string(), id);
    }

    /// Return a `BeliefNode` by key, or `None` if not found.
    pub fn get(&self, key: &str) -> Option<PyBeliefNode> {
        let state = self.state.read().unwrap();
        let id = *state.key_to_id.get(key)?;
        state.quad.get(id).map(node_to_py)
    }

    /// Remove a belief by key. Returns `True` if the belief existed.
    pub fn remove(&self, key: &str) -> bool {
        let mut state = self.state.write().unwrap();
        if let Some(id) = state.key_to_id.remove(key) {
            state.quad.remove(id).is_some()
        } else {
            false
        }
    }

    // ── AGM revision ──────────────────────────────────────────────────────────

    /// Revise the belief at `key` to hold `new_value` with the given `confidence`.
    ///
    /// Implements the Levi identity: contract-then-expand when the new value
    /// contradicts the current state; expand-only otherwise. Satisfies K*2–K*6.
    ///
    /// Raises `BeliefRevisionError` if the key does not exist in the quad.
    #[pyo3(signature = (key, new_value, confidence, provenance = None, llm_model = None, tool_name = None))]
    pub fn revise(
        &self,
        key: &str,
        new_value: &str,
        confidence: f32,
        provenance: Option<&str>,
        llm_model: Option<&str>,
        tool_name: Option<&str>,
    ) -> PyResult<()> {
        let prov = parse_provenance(provenance, llm_model, tool_name);
        let mut state = self.state.write().unwrap();
        let id = state
            .key_to_id
            .get(key)
            .copied()
            .ok_or_else(|| BeliefRevisionError::new_err(format!("belief key not found: {key:?}")))?;
        state.quad.revise(id, BeliefValue::Asserted(new_value.to_string()), prov, confidence)
            .map_err(map_revision_err)?;
        Ok(())
    }

    // ── Checkpoints ───────────────────────────────────────────────────────────

    /// Save the current quad state and return a checkpoint ID string.
    pub fn checkpoint(&self) -> String {
        self.state.write().unwrap().quad.checkpoint().to_string()
    }

    /// Roll back to a previously saved checkpoint.
    ///
    /// Returns a `BeliefDiff` describing what changed. Raises `CheckpointError`
    /// if the checkpoint is not found or if the K*4 guard determines the rollback
    /// is unnecessary (no actual contradictions to resolve).
    pub fn rollback_to(&self, checkpoint_id: &str) -> PyResult<PyBeliefDiff> {
        let cp_id = parse_checkpoint_id(checkpoint_id)?;
        let mut state = self.state.write().unwrap();
        let diff = state.quad.rollback_to(cp_id).map_err(map_rollback_err)?;
        state.rebuild_index();
        Ok(diff_to_py(diff))
    }

    /// Return a list of all stored checkpoint ID strings.
    pub fn list_checkpoints(&self) -> Vec<String> {
        self.state
            .read()
            .unwrap()
            .quad
            .checkpoints()
            .iter()
            .map(|c| c.id.to_string())
            .collect()
    }

    /// Compute the structural diff between the current state and a checkpoint.
    ///
    /// `added` = beliefs added since checkpoint.
    /// `removed` = beliefs removed since checkpoint.
    /// `modified` = beliefs whose value or confidence changed.
    pub fn diff_with_checkpoint(&self, checkpoint_id: &str) -> PyResult<PyBeliefDiff> {
        let cp_id = parse_checkpoint_id(checkpoint_id)?;
        let state = self.state.read().unwrap();
        let cp = state.quad.checkpoint_by_id(cp_id)
            .ok_or_else(|| CheckpointError::new_err(format!("checkpoint not found: {checkpoint_id}")))?;
        let diff = state.quad.diff(cp.quad.as_ref());
        Ok(diff_to_py(diff))
    }

    // ── Counterfactual ────────────────────────────────────────────────────────

    /// Return the keys of all beliefs that would survive if `key` had never been inserted.
    ///
    /// Computes the set of causal descendants of the given belief and returns
    /// the surviving keys (beliefs not in the excluded set). No LLM calls —
    /// pure traversal of the CausalGraph.
    ///
    /// Raises `BeliefRevisionError` if the key does not exist.
    pub fn counterfactual(&self, key: &str) -> PyResult<PyCounterfactualResult> {
        let state = self.state.read().unwrap();
        let id = state
            .key_to_id
            .get(key)
            .copied()
            .ok_or_else(|| BeliefRevisionError::new_err(format!("belief key not found: {key:?}")))?;
        let world = state.quad.counterfactual_query(id);
        let surviving: Vec<String> = world.surviving_beliefs().map(|(_, n)| n.key.clone()).collect();
        let excluded_count = world.excluded.len();
        Ok(PyCounterfactualResult {
            removed_key: key.to_string(),
            surviving,
            excluded_count,
        })
    }

    // ── Graph edges ───────────────────────────────────────────────────────────

    /// Add a directed semantic edge between two beliefs.
    ///
    /// `edge_type` is one of `"subsumes"`, `"contradicts"`, or `"synonymous"`.
    pub fn add_semantic_edge(&self, from_key: &str, to_key: &str, edge_type: &str) -> PyResult<()> {
        let edge = match edge_type {
            "subsumes"   => SemanticEdge::Subsumes,
            "contradicts" => SemanticEdge::Contradicts,
            "synonymous" => SemanticEdge::Synonymous,
            other => return Err(pyo3::exceptions::PyValueError::new_err(
                format!("unknown semantic edge type: {other:?}; use 'subsumes', 'contradicts', or 'synonymous'")
            )),
        };
        let mut state = self.state.write().unwrap();
        let from = state.key_to_id.get(from_key).copied()
            .ok_or_else(|| pyo3::exceptions::PyKeyError::new_err(format!("key not found: {from_key:?}")))?;
        let to = state.key_to_id.get(to_key).copied()
            .ok_or_else(|| pyo3::exceptions::PyKeyError::new_err(format!("key not found: {to_key:?}")))?;
        state.quad.add_semantic_edge(from, to, edge);
        Ok(())
    }

    /// Add a directed causal edge: `cause_key` → `effect_key`.
    ///
    /// `effect_size` and `confidence` are optional and default to `1.0`.
    #[pyo3(signature = (cause_key, effect_key, effect_size = 1.0, confidence = 1.0))]
    pub fn add_causal_edge(
        &self,
        cause_key: &str,
        effect_key: &str,
        effect_size: f32,
        confidence: f32,
    ) -> PyResult<()> {
        let mut state = self.state.write().unwrap();
        let from = state.key_to_id.get(cause_key).copied()
            .ok_or_else(|| pyo3::exceptions::PyKeyError::new_err(format!("key not found: {cause_key:?}")))?;
        let to = state.key_to_id.get(effect_key).copied()
            .ok_or_else(|| pyo3::exceptions::PyKeyError::new_err(format!("key not found: {effect_key:?}")))?;
        state.quad.add_causal_edge(from, to, CausalEdge::Causes { effect_size, confidence });
        Ok(())
    }

    /// Add a directed temporal edge: `earlier_key` → `later_key`.
    pub fn add_temporal_edge(&self, earlier_key: &str, later_key: &str) -> PyResult<()> {
        let edge = TemporalEdge::Precedes { gap_ms: 0 };
        let mut state = self.state.write().unwrap();
        let from = state.key_to_id.get(earlier_key).copied()
            .ok_or_else(|| pyo3::exceptions::PyKeyError::new_err(format!("key not found: {earlier_key:?}")))?;
        let to = state.key_to_id.get(later_key).copied()
            .ok_or_else(|| pyo3::exceptions::PyKeyError::new_err(format!("key not found: {later_key:?}")))?;
        state.quad.add_temporal_edge(from, to, edge);
        Ok(())
    }

    // ── Bulk accessors ────────────────────────────────────────────────────────

    /// Return all belief keys as a list.
    pub fn keys(&self) -> Vec<String> {
        self.state.read().unwrap().quad.iter().map(|(_, n)| n.key.clone()).collect()
    }

    /// Return all beliefs as a list of `BeliefNode` objects.
    pub fn values(&self) -> Vec<PyBeliefNode> {
        self.state.read().unwrap().quad.iter().map(|(_, n)| node_to_py(n)).collect()
    }

    /// Return all `(key, BeliefNode)` pairs.
    pub fn items(&self) -> Vec<(String, PyBeliefNode)> {
        self.state
            .read()
            .unwrap()
            .quad
            .iter()
            .map(|(_, n)| (n.key.clone(), node_to_py(n)))
            .collect()
    }

    // ── Dunder methods ────────────────────────────────────────────────────────

    fn __len__(&self) -> usize {
        self.state.read().unwrap().quad.len()
    }

    fn __contains__(&self, key: &str) -> bool {
        self.state.read().unwrap().key_to_id.contains_key(key)
    }

    fn __repr__(&self) -> String {
        let state = self.state.read().unwrap();
        format!(
            "BeliefQuad(beliefs={n}, checkpoints={c}, version={v})",
            n = state.quad.len(),
            c = state.quad.checkpoint_count(),
            v = state.quad.version(),
        )
    }

    fn __bool__(&self) -> bool {
        !self.state.read().unwrap().quad.is_empty()
    }

    fn is_empty(&self) -> bool {
        self.state.read().unwrap().quad.is_empty()
    }

    fn len(&self) -> usize {
        self.state.read().unwrap().quad.len()
    }

    // ── Dict protocol ─────────────────────────────────────────────────────────

    fn __getitem__(&self, key: &str) -> PyResult<PyBeliefNode> {
        self.get(key).ok_or_else(|| {
            pyo3::exceptions::PyKeyError::new_err(format!("belief key not found: {key:?}"))
        })
    }

    /// Inserts with `confidence=1.0` and `provenance="user"`.
    /// For fine-grained control use `insert()` or `revise()` directly.
    fn __setitem__(&self, key: &str, value: &str) {
        self.insert(key, value, 1.0, None, None, None);
    }

    fn __delitem__(&self, key: &str) -> PyResult<()> {
        if !self.remove(key) {
            return Err(pyo3::exceptions::PyKeyError::new_err(format!(
                "belief key not found: {key:?}"
            )));
        }
        Ok(())
    }

    fn __iter__(&self) -> Vec<String> {
        self.keys()
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn parse_provenance(kind: Option<&str>, model: Option<&str>, tool: Option<&str>) -> Provenance {
    match kind {
        Some("llm") => Provenance::LlmInference {
            model: model.unwrap_or("unknown").to_string(),
            call_id: uuid::Uuid::new_v4(),
            prompt_hash: 0,
        },
        Some("tool") => Provenance::ToolResult {
            tool: tool.unwrap_or("unknown").to_string(),
            call_id: uuid::Uuid::new_v4(),
        },
        _ => Provenance::UserStatement { turn: 0 },
    }
}

fn parse_checkpoint_id(s: &str) -> PyResult<CheckpointId> {
    CheckpointId::parse(s)
        .ok_or_else(|| CheckpointError::new_err(format!("invalid checkpoint ID: {s:?}")))
}

/// Public re-export used by `runtime.rs`.
pub(crate) fn parse_checkpoint_id_pub(s: &str) -> PyResult<CheckpointId> {
    parse_checkpoint_id(s)
}
