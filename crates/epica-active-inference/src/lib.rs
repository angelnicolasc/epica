//! # epica-active-inference
//!
//! Continuous Bayesian-surprise monitor for [`BeliefQuad`]
//! ([`epica_core::BeliefQuad`]), grounded in Karl Friston's Free Energy
//! Principle (FEP) and the variational implementation lineage of
//! [pymdp](https://github.com/infer-actively/pymdp) /
//! [RxInfer.jl](https://github.com/biaslab/RxInfer.jl).
//!
//! ## Why
//!
//! Epica's behavioral contracts ([`epica_contracts`]) are *point-in-time*
//! gates: a `BehavioralContract` evaluates preconditions and invariants at
//! mutation time and either allows or rejects the mutation. That model is
//! reactive — it can stop a single bad write, but it can't notice an agent
//! drifting steadily into a state that *none* of the individual writes
//! violate.
//!
//! Active Inference reframes the question. The runtime models the agent as
//! an organism that maintains a generative model `p(o, s)` of the world,
//! and a posterior `q(s)` over hidden states. The agent's *surprise* on
//! each observation is bounded above by the *variational free energy*
//!
//! ```text
//! F(q, o) = E_q[ln q(s) - ln p(o, s)]
//!         = D_KL[q(s) || p(s)]  -  E_q[ln p(o|s)]
//! ```
//!
//! Tracking `F` over time gives a continuous, model-agnostic signal: when
//! `F` rises above a budget the agent is no longer in homeostasis — its
//! beliefs and observations no longer agree with the world it modeled.
//! That signal is independent of which contract fires; it's the *whole-
//! agent* read.
//!
//! ## Honest scope
//!
//! Friston's FEP is sustrate-agnostic. The Sprint 2 implementation maps
//! `q(s)` to the **`BeliefQuad`**, not to the LLM's internal activations:
//!
//! - **Hidden state `s_i`** of belief `i` is its truth value
//!   (`true` / `false`).
//! - **Posterior `q_i(s_i = true) = c_i`** — the belief's `fast_confidence`.
//! - **Prior `p_i(s_i = true) = π_i`** — derived from the causal graph: the
//!   mean confidence of `b_i`'s direct causal parents, or `0.5` when there
//!   is no causal predecessor.
//! - **Observation likelihood** — for the most recent observation
//!   (`last_obs`), `p(o = true | s = true) = c_obs` and `p(o = true | s =
//!   false) = 1 - c_obs`. This treats the observation's reported
//!   confidence as the inverse of its measurement noise.
//!
//! This is honest: the FEP doesn't require mapping the LLM's internals —
//! it only requires a consistent posterior/prior/likelihood at *some*
//! level of abstraction. The `BeliefQuad` is the level Epica already
//! reasons about; using it keeps the math derivable, the runtime
//! independent of any specific LLM, and the cost bounded.
//!
//! See `docs/active-inference.md` (forthcoming) for the full derivation
//! and the relationship to the AGM postulates already implemented in
//! `epica-core`.
//!
//! ## What it does *not* do
//!
//! - Does **not** block mutations by default. `observe()` returns a
//!   [`SurpriseSignal`]; the caller decides what to do with it. Wiring it
//!   into `BehavioralContract` as a precondition is a single extra line
//!   on the runtime side (see `epica-runtime` feature `active-inference`).
//! - Does **not** assume a specific LLM. The monitor reads only the
//!   `BeliefQuad`'s `fast_confidence` and causal edges.
//! - Does **not** modify the quad. `observe()` takes `&BeliefQuad`.
//!
//! ## Quick start
//!
//! ```rust
//! use epica_active_inference::{ActiveInferenceMonitor, MonitorConfig};
//! use epica_core::{BeliefNode, BeliefQuad, BeliefValue, Provenance};
//!
//! let mut quad = BeliefQuad::new();
//! let id = quad.insert(BeliefNode::new(
//!     "user_intent",
//!     BeliefValue::Asserted("refactor auth".into()),
//!     Provenance::UserStatement { turn: 0 },
//!     0.9,
//! ));
//! let last = quad.get(id).unwrap().clone();
//!
//! let mut monitor = ActiveInferenceMonitor::with_config(MonitorConfig {
//!     surprise_threshold: 3.0,
//!     homeostatic_budget: 10.0,
//!     history_capacity: 256,
//! });
//! let signal = monitor.observe(&quad, &last);
//!
//! // Free energy is finite, and within budget on the first observation.
//! assert!(signal.free_energy.is_finite());
//! assert!(!signal.exceeds_budget);
//! ```

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod config;
pub mod free_energy;
pub mod monitor;

pub use config::MonitorConfig;
pub use free_energy::{
    bernoulli_kl, expected_log_likelihood, quad_free_energy, FreeEnergyBreakdown,
};
pub use monitor::{ActiveInferenceMonitor, SurpriseSignal};
