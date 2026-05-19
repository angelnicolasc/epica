//! # epica-benchmarks
//!
//! Workspace-internal benchmark harness for Epica.
//!
//! ## What this crate ships *today*
//!
//! - [`traces`]: deterministic, seedable trajectory generators that
//!   emulate the *epistemic shape* of two canonical agent benchmarks:
//!     - **ALFWorld-style**: multi-step goal pursuit ("find object,
//!       place in container") — beliefs are inserted, refined,
//!       contradicted by intermediate observations, and finally
//!       resolved.
//!     - **WebShop-style**: search-then-filter-then-purchase —
//!       candidates inserted, contradicted by filters, paraphrases of
//!       the user intent appear across turns.
//! - [`metrics`]: per-trajectory and aggregate computation of the four
//!   metrics the Sprint-4 plan asks for: **BeliefShift / T-ECE**,
//!   **contract violations**, **free-energy mean** (when the FEP
//!   monitor is wired), and **insert latency p50/p99**.
//! - [`harness`]: drives traces through a configured
//!   [`BeliefRuntime`][epica_runtime::BeliefRuntime] and collects
//!   metrics.
//! - [`reporters`]: emits CSV + Markdown summaries fit for
//!   `docs/benchmarks/`.
//! - [`epica-bench`][1]: CLI entry point.
//!
//! [1]: https://github.com/angelnicolasc/epica
//!
//! ## What this crate does *not* ship today
//!
//! The Sprint-4 plan mentions "ALFWorld (text) and WebShop (web
//! simulated)" via "pyo3 inverso" — Rust spawning a Python subprocess
//! that hosts the simulator and an LLM. That harness is heavier than
//! the rest of Sprint 4 combined (Python env, AI2Thor, Flask sim,
//! LLM API budget per trajectory) and would land *real* trajectories
//! at the cost of CI portability.
//!
//! This crate keeps the *epistemic shape* of those tasks — paraphrase
//! handling, contradiction sequences, multi-step goal coherence — but
//! drives them from a deterministic generator so:
//!
//! 1. The benchmark is reproducible bit-for-bit across runs and
//!    machines.
//! 2. No network, no Python, no LLM cost.
//! 3. The reported numbers are not "what an LLM did today against
//!    Epica"; they are "what Epica's runtime does against
//!    representative trajectory patterns."
//!
//! See [`real_adapters`] for the documented skeleton of the
//! real-environment adapters and what wiring them up would entail.
//!
//! ## Quick start
//!
//! ```rust,no_run
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! use epica_benchmarks::{
//!     harness::HarnessConfig,
//!     run_suite, Suite,
//! };
//!
//! let report = run_suite(Suite::AlfworldLike, 50, &HarnessConfig::default()).await;
//! println!("T-ECE: {:?}", report.metrics.tece);
//! println!("p99 insert latency: {} µs", report.metrics.insert_latency_us.p99);
//! # Ok(())
//! # }
//! ```

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod harness;
pub mod metrics;
pub mod real_adapters;
pub mod reporters;
pub mod traces;

pub use harness::{run_suite, HarnessConfig, HarnessError, SuiteReport};
pub use metrics::{LatencyStats, MetricSet};
pub use traces::{Suite, Trace, TraceStep};
