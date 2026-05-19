//! PyO3 Python bindings for Epica.
//!
//! Build with maturin: `maturin develop` from `crates/epica-python/`.
//! Install for development: `pip install -e ".[dev]"`.
//!
//! The module is exposed as `epica._epica`. The `python/epica/__init__.py`
//! re-exports all public names for the user-facing `import epica` experience.

pub mod belief;
pub mod contracts;
pub mod error;
pub mod llm_client;
pub mod query;
pub mod runtime;
pub mod session;
pub mod types;

use pyo3::prelude::*;

use belief::PyBeliefQuad;
use contracts::PyBehavioralContract;
use llm_client::{PyLlmClientHandle, PyMockLlmClient};
use query::{PyBeliefDiff, PyCounterfactualResult};
use runtime::PyBeliefRuntime;
use session::PySessionReport;
use types::PyBeliefNode;

/// Register the `epica._epica` Python module.
#[pymodule]
fn _epica(m: &Bound<'_, PyModule>) -> PyResult<()> {
    // ── Exception hierarchy ───────────────────────────────────────────────────
    error::register(m)?;

    // ── Data classes ──────────────────────────────────────────────────────────
    m.add_class::<PyBeliefNode>()?;
    m.add_class::<PyBeliefDiff>()?;
    m.add_class::<PyCounterfactualResult>()?;
    m.add_class::<PySessionReport>()?;

    // ── Core runtime ──────────────────────────────────────────────────────────
    m.add_class::<PyBeliefQuad>()?;
    m.add_class::<PyBeliefRuntime>()?;
    m.add_class::<PyBehavioralContract>()?;

    // ── LLM client injection (TD-P7-002) ──────────────────────────────────────
    m.add_class::<PyLlmClientHandle>()?;
    m.add_class::<PyMockLlmClient>()?;

    // ── Package metadata ──────────────────────────────────────────────────────
    m.add("__version__", "0.6.0")?;
    m.add("__author__", "Epica Contributors")?;

    Ok(())
}
