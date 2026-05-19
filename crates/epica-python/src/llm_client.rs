//! Python bindings for `epica_runtime::LlmClient` injection.
//!
//! Closes TD-P7-002. Two types are exposed:
//!
//! - [`PyLlmClientHandle`] — opaque handle wrapping `Arc<dyn LlmClient>`. Pass
//!   it to [`crate::runtime::PyBeliefRuntime::attach_llm_client`].
//! - [`PyMockLlmClient`] — a deterministic, in-Python-process `LlmClient`
//!   used by tests and notebooks to exercise the System 2 path without a
//!   network call. Its [`PyMockLlmClient::handle`] method returns a
//!   `PyLlmClientHandle` ready to attach to a runtime.
//!
//! A future commit can ship an `AnthropicLlmClient` Python binding the same
//! way: implement `LlmClient` in Rust, wrap it in `Arc`, and expose a
//! `.handle()` method that hands ownership to `PyLlmClientHandle`.

use std::sync::Arc;

use pyo3::prelude::*;

use epica_runtime::{DiagnosticSignal, LlmClient, LlmClientError, System2Result};

/// Opaque handle that owns an `Arc<dyn LlmClient>`.
///
/// Constructed by concrete client implementations (`PyMockLlmClient::handle`,
/// future `PyAnthropicLlmClient::handle`, …) and consumed by
/// `PyBeliefRuntime::attach_llm_client`.
#[pyclass(name = "LlmClientHandle", module = "epica")]
pub struct PyLlmClientHandle {
    pub(crate) inner: Arc<dyn LlmClient>,
}

impl PyLlmClientHandle {
    pub fn new(client: Arc<dyn LlmClient>) -> Self {
        Self { inner: client }
    }
}

#[pymethods]
impl PyLlmClientHandle {
    fn __repr__(&self) -> String {
        "LlmClientHandle(<opaque>)".to_string()
    }
}

/// Deterministic mock `LlmClient` for tests and notebooks.
///
/// `reflect()` returns the configured `revised_confidence` regardless of the
/// incoming `DiagnosticSignal`, and tracks how many times it has been called.
/// The mock is intentionally minimal — exhaustive `LlmClient` mocks belong in
/// the test suite, not in the public SDK.
#[pyclass(name = "MockLlmClient", module = "epica")]
pub struct PyMockLlmClient {
    inner: Arc<MockState>,
}

struct MockState {
    revised_confidence: f32,
    reasoning: String,
    calls: std::sync::atomic::AtomicU64,
}

#[async_trait::async_trait]
impl LlmClient for MockState {
    async fn reflect(
        &self,
        _diagnostic: &DiagnosticSignal,
    ) -> Result<System2Result, LlmClientError> {
        self.calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(System2Result {
            revised_confidence: self.revised_confidence,
            reasoning: self.reasoning.clone(),
        })
    }
}

#[pymethods]
impl PyMockLlmClient {
    #[new]
    #[pyo3(signature = (revised_confidence = 0.5, reasoning = "mock reflection"))]
    fn new(revised_confidence: f32, reasoning: &str) -> Self {
        Self {
            inner: Arc::new(MockState {
                revised_confidence,
                reasoning: reasoning.to_string(),
                calls: std::sync::atomic::AtomicU64::new(0),
            }),
        }
    }

    /// Number of `reflect()` calls observed since construction.
    #[getter]
    fn call_count(&self) -> u64 {
        self.inner
            .calls
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Hand out an `LlmClientHandle` referring to this mock.
    ///
    /// The mock and the handle share the same underlying state, so
    /// `call_count` continues to reflect calls made through the runtime.
    fn handle(&self) -> PyLlmClientHandle {
        PyLlmClientHandle::new(self.inner.clone() as Arc<dyn LlmClient>)
    }

    fn __repr__(&self) -> String {
        format!(
            "MockLlmClient(revised_confidence={}, calls={})",
            self.inner.revised_confidence,
            self.call_count()
        )
    }
}
