//! `PyBeliefRuntime` — dual-process runtime wrapper for Python.
//!
//! Bridges Python's synchronous call model to Rust's async `BeliefRuntime` via
//! a dedicated Tokio runtime. Internal state is wrapped in `Arc<Mutex<>>` so
//! the object can be safely shared across GIL-free Python 3.13+ threads.

use std::sync::{Arc, Mutex};

use pyo3::prelude::*;

use epica_core::{BeliefNode, BeliefQuad, BeliefValue, Provenance};
use epica_runtime::{BeliefRuntime, RuntimeUpdateResult};

use crate::error::{map_rollback_err, map_runtime_err, EpicaError};
use crate::query::{diff_to_py, PyBeliefDiff};
use crate::session::{report_to_py, PySessionReport};
use crate::types::{node_to_py, PyBeliefNode};

// ── Internal state ────────────────────────────────────────────────────────────

struct RuntimeState {
    inner: BeliefRuntime,
    rt: tokio::runtime::Runtime,
}

// ── PyBeliefRuntime ───────────────────────────────────────────────────────────

/// Async `BeliefRuntime` wrapped for synchronous Python access.
///
/// Orchestrates System 1 (fast, Noisy-OR propagation) and System 2 (slow,
/// LLM-based reflection) over a `BeliefQuad`. Supports behavioral contracts,
/// session reporting, and Trajectory-ECE calibration measurement.
///
/// ## Usage
/// ```python
/// from epica import BeliefRuntime
///
/// with BeliefRuntime(reflection_threshold=0.15, budget=50) as rt:
///     rt.insert_belief("user_goal", "deploy service", 0.9)
///     rt.insert_belief("blocker", "auth failing", 0.7)
///
///     id_str = rt.get_by_key("user_goal")
///     rt.update_belief("user_goal", "deploy cancelled", 0.3)
///
///     report = rt.finalize_session()
///     print(report.trajectory_ece)
/// ```
///
/// ## Thread safety
/// Internally wrapped in `Arc<Mutex<>>` — safe for use from GIL-free Python
/// 3.13+ threads. Calls are serialised by the Mutex.
#[pyclass(name = "BeliefRuntime")]
pub struct PyBeliefRuntime {
    state: Arc<Mutex<RuntimeState>>,
}

#[pymethods]
impl PyBeliefRuntime {
    // ── Constructor ───────────────────────────────────────────────────────────

    #[new]
    #[pyo3(signature = (reflection_threshold = 0.15, budget = 50, refill_rate = 1.0))]
    pub fn new(reflection_threshold: f32, budget: u32, refill_rate: f32) -> PyResult<Self> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| EpicaError::new_err(e.to_string()))?;
        let inner = BeliefRuntime::new(BeliefQuad::new(), reflection_threshold, budget, refill_rate);
        Ok(Self {
            state: Arc::new(Mutex::new(RuntimeState { inner, rt })),
        })
    }

    // ── Belief operations ─────────────────────────────────────────────────────

    /// Insert a new belief. Returns the key (the stable Python-facing ID).
    ///
    /// `provenance` is one of `"user"` (default), `"llm"`, or `"tool"`.
    /// `llm_model` names the LLM when `provenance="llm"` (e.g. `"claude-opus-4-7"`).
    /// `tool_name` names the tool when `provenance="tool"` (e.g. `"bash"`).
    #[pyo3(signature = (key, value, confidence, provenance = None, llm_model = None, tool_name = None))]
    pub fn insert_belief(
        &self,
        key: &str,
        value: &str,
        confidence: f32,
        provenance: Option<&str>,
        llm_model: Option<&str>,
        tool_name: Option<&str>,
    ) -> String {
        let prov = parse_provenance(provenance, llm_model, tool_name);
        let node = BeliefNode::new(key, BeliefValue::Asserted(value.to_string()), prov, confidence);
        let mut state = self.state.lock().unwrap();
        let RuntimeState { inner, rt } = &mut *state;
        rt.block_on(inner.insert_belief(node));
        key.to_string()
    }

    /// Look up the internal ID of a belief by key. Returns `None` if not found.
    ///
    /// The returned string is opaque — pass it back to `update_belief()`.
    pub fn get_by_key(&self, key: &str) -> Option<String> {
        let mut state = self.state.lock().unwrap();
        let RuntimeState { inner, rt } = &mut *state;
        rt.block_on(inner.get_by_key(key)).map(|id| format!("{id:?}"))
    }

    /// Look up a belief node snapshot by key.
    pub fn get_belief(&self, key: &str) -> Option<PyBeliefNode> {
        let mut state = self.state.lock().unwrap();
        let RuntimeState { inner, rt } = &mut *state;
        let id = rt.block_on(inner.get_by_key(key))?;
        let quad = rt.block_on(inner.read_quad());
        quad.get(id).map(node_to_py)
    }

    /// Update a belief, run System 1, and conditionally activate System 2.
    ///
    /// Returns a dict with:
    /// - `"status"`: `"system1_only"` | `"system2_activated"` | `"system2_throttled"`
    /// - `"task_id"`: task UUID string when `"system2_activated"`, else `None`
    ///
    /// Raises:
    /// - `KeyError` if the key does not exist.
    /// - `ContractViolationError` if a behavioral contract precondition or invariant fails.
    #[pyo3(signature = (key, new_value, confidence, provenance = None, llm_model = None, tool_name = None))]
    pub fn update_belief(
        &self,
        key: &str,
        new_value: &str,
        confidence: f32,
        provenance: Option<&str>,
        llm_model: Option<&str>,
        tool_name: Option<&str>,
    ) -> PyResult<PyObject> {
        let prov = parse_provenance(provenance, llm_model, tool_name);
        let mut state = self.state.lock().unwrap();
        let RuntimeState { inner, rt } = &mut *state;

        let id = rt.block_on(inner.get_by_key(key))
            .ok_or_else(|| pyo3::exceptions::PyKeyError::new_err(format!("belief key not found: {key:?}")))?;

        let result = rt.block_on(inner.update_belief(
            id,
            BeliefValue::Asserted(new_value.to_string()),
            prov,
            confidence,
        )).map_err(map_runtime_err)?;

        Python::with_gil(|py| {
            let dict = pyo3::types::PyDict::new_bound(py);
            match result {
                RuntimeUpdateResult::System1Only => {
                    dict.set_item("status", "system1_only")?;
                    dict.set_item("task_id", py.None())?;
                }
                RuntimeUpdateResult::System2Activated { .. } => {
                    dict.set_item("status", "system2_activated")?;
                    dict.set_item("task_id", py.None())?; // task_id managed by MCP layer
                }
                RuntimeUpdateResult::System2Pending { .. } => {
                    // S-2: update_belief() now returns Pending; consumption of the
                    // budget, the LLM call, and apply_system2_result() are the
                    // caller's responsibility. The Python binding does not drive
                    // System 2 directly — surface the status to user code.
                    dict.set_item("status", "system2_pending")?;
                    dict.set_item("task_id", py.None())?;
                }
                RuntimeUpdateResult::System2Throttled => {
                    dict.set_item("status", "system2_throttled")?;
                    dict.set_item("task_id", py.None())?;
                }
            }
            Ok(dict.into())
        })
    }

    /// Retrieve beliefs most relevant to `query` within a token `budget`.
    ///
    /// Returns a list of `(key, confidence)` tuples sorted by multicriteria score:
    /// `prospective_sim×0.45 + uncertainty_bonus×0.25 + causal_centrality×0.20 − decay×0.10`.
    ///
    /// `budget` approximates 100 tokens per belief (default: 1000 tokens → top 10).
    #[pyo3(signature = (query, budget = 1000))]
    pub fn retrieve_for_query(&self, query: &str, budget: usize) -> Vec<(String, f32)> {
        let mut state = self.state.lock().unwrap();
        let RuntimeState { inner, rt } = &mut *state;
        let snapshots = rt.block_on(inner.retrieve_for_query(query, budget));
        snapshots.into_iter().map(|s| (s.key, s.fast_confidence)).collect()
    }

    // ── Checkpoints ───────────────────────────────────────────────────────────

    /// Create a checkpoint of the current quad state. Returns a checkpoint ID string.
    pub fn checkpoint(&self) -> String {
        let mut state = self.state.lock().unwrap();
        let RuntimeState { inner, rt } = &mut *state;
        rt.block_on(inner.checkpoint()).to_string()
    }

    /// Roll back to a previously saved checkpoint.
    ///
    /// Raises `CheckpointError` if the checkpoint is not found or the K*4 guard
    /// determines the rollback would unnecessarily contract beliefs.
    pub fn rollback_to(&self, checkpoint_id: &str) -> PyResult<PyBeliefDiff> {
        let cp_id = crate::belief::parse_checkpoint_id_pub(checkpoint_id)?;
        let mut state = self.state.lock().unwrap();
        let RuntimeState { inner, rt } = &mut *state;
        let diff = rt.block_on(inner.rollback_to(cp_id)).map_err(map_rollback_err)?;
        Ok(diff_to_py(diff))
    }

    // ── Session ───────────────────────────────────────────────────────────────

    /// Mark all surviving beliefs as correct and return the session report.
    ///
    /// Call this once at the end of a session. The returned `SessionReport`
    /// includes Trajectory-ECE, System 2 activation counts, and contract metrics.
    pub fn finalize_session(&self) -> PySessionReport {
        let mut state = self.state.lock().unwrap();
        let RuntimeState { inner, rt } = &mut *state;
        rt.block_on(inner.finalize_session());
        let report = rt.block_on(inner.session_report());
        report_to_py(report)
    }

    /// Return the current session report without finalizing.
    pub fn session_report(&self) -> PySessionReport {
        let mut state = self.state.lock().unwrap();
        let RuntimeState { inner, rt } = &mut *state;
        let report = rt.block_on(inner.session_report());
        report_to_py(report)
    }

    // ── Context manager ───────────────────────────────────────────────────────

    fn __enter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __exit__(
        &self,
        _exc_type: Option<Bound<'_, PyAny>>,
        _exc_value: Option<Bound<'_, PyAny>>,
        _traceback: Option<Bound<'_, PyAny>>,
    ) -> bool {
        // Finalize the session on context manager exit (best-effort; errors suppressed).
        let mut state = self.state.lock().unwrap();
        let RuntimeState { inner, rt } = &mut *state;
        rt.block_on(inner.finalize_session());
        false // do not suppress exceptions
    }

    // ── Dunder methods ────────────────────────────────────────────────────────

    fn __repr__(&self) -> String {
        let mut state = self.state.lock().unwrap();
        let RuntimeState { inner, rt } = &mut *state;
        let activations = inner.system2_activations();
        let quad_len = rt.block_on(async { inner.read_quad().await.len() });
        format!("BeliefRuntime(beliefs={quad_len}, system2_activations={activations})")
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
