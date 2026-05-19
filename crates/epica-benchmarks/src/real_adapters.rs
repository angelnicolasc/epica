//! Skeleton for real-environment adapters.
//!
//! ## Status
//!
//! The Sprint-4 plan called for two adapters that drive Epica's
//! [`BeliefRuntime`][epica_runtime::BeliefRuntime] against the
//! *actual* ALFWorld and WebShop simulators:
//!
//! - **ALFWorld** ships as a Python package backed by AI2-THOR; the
//!   adapter would spawn a Python subprocess that loops on `step`
//!   actions, ferrying observations back over a JSON-RPC channel.
//! - **WebShop** ships as a Flask application that wraps a snapshot
//!   of an e-commerce catalogue; the adapter would HTTP-poll the
//!   simulator endpoints.
//!
//! Both adapters depend on infra that is hard to provision inside a
//! Rust workspace CI: a Python ≥ 3.10 env with AI2-THOR + alfworld
//! installed (multi-hundred-MB), and a long-running Flask server with
//! a snapshot DB for WebShop. That's not something a portfolio reader
//! should have to set up before they trust the benchmark numbers.
//!
//! The Sprint-4 shipping decision: **report numbers from the
//! synthetic [`super::traces`] generators today** (deterministic,
//! reproducible, zero infra), and reserve this module for the real
//! adapters once the workspace acquires a CI runner with Python +
//! AI2-THOR + a WebShop fixture image.
//!
//! ## What lands here
//!
//! The [`RealEnvAdapter`] trait is the seam. A concrete
//! `AlfworldAdapter` and `WebshopAdapter` will each:
//!
//! 1. `start()`: bring up the environment (Python subprocess, HTTP
//!    client + snapshot).
//! 2. `step(action) -> Observation`: pump one action through the
//!    simulator; the harness re-routes the observation back into
//!    [`BeliefRuntime::insert_belief`] / `update_belief`.
//! 3. `is_done() -> bool`: terminal predicate.
//! 4. `shutdown()`: clean up the subprocess / HTTP client.
//!
//! Because the [`crate::harness`] is already structured around `Trace`
//! → runtime, the wiring is a thin translation layer: each real-env
//! step yields a `TraceStep` synthesised from the observation, and
//! the harness consumes it the same way.

/// Generic seam for a real-environment adapter.
///
/// Today this trait is **not** implemented anywhere in the workspace.
/// It exists so the harness can target it from day one and the only
/// missing piece is the concrete adapter — there's no churn on the
/// harness API when the real adapters land.
#[allow(dead_code)]
pub trait RealEnvAdapter {
    /// Observation type — a serialisable summary of the
    /// environment's response to one step.
    type Observation: serde::Serialize + serde::de::DeserializeOwned;
    /// Action type — what the harness sends per step.
    type Action: serde::Serialize;
    /// Errors raised by the adapter.
    type Error: std::error::Error + Send + Sync + 'static;

    /// Bring up the environment.
    fn start(&mut self) -> Result<(), Self::Error>;
    /// Execute one action; return the resulting observation.
    fn step(&mut self, action: Self::Action) -> Result<Self::Observation, Self::Error>;
    /// Whether the environment has reached a terminal state.
    fn is_done(&self) -> bool;
    /// Clean up.
    fn shutdown(&mut self) -> Result<(), Self::Error>;
}

/// Compile-time marker: no real adapter is wired today.
/// When one ships, this constant flips to `true` and the binary
/// learns to dispatch on suite names like `alfworld_real` /
/// `webshop_real`.
pub const REAL_ADAPTERS_AVAILABLE: bool = false;
