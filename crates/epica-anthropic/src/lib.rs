//! # epica-anthropic
//!
//! [`AnthropicLlmClient`]: a production `LlmClient` implementation for Epica's
//! System 2 (UAR — Uncertainty-Aware Reflection, arXiv:2601.15703) using the
//! Anthropic Messages API.
//!
//! ## Design
//!
//! System 2 recalibrates a belief's confidence when System 1 diverges
//! significantly from the agent's reliability baseline.  This crate sends a
//! structured reflection request to Claude via the Anthropic tool_use API,
//! forcing the model to return `revised_confidence` and `reasoning` as typed
//! JSON — no text parsing, no ambiguity.
//!
//! ## Quick start
//!
//! ```rust,no_run
//! use epica_anthropic::AnthropicLlmClient;
//! use epica_core::BeliefQuad;
//! use epica_runtime::BeliefRuntime;
//! use std::sync::Arc;
//!
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! // Reads ANTHROPIC_API_KEY from the environment
//! let client = AnthropicLlmClient::from_env()?;
//!
//! let rt = BeliefRuntime::new(BeliefQuad::new(), 0.5, 20, 2.0)
//!     .with_llm_client(Arc::new(client));
//! # Ok(())
//! # }
//! ```

#![warn(missing_docs)]

pub mod client;
pub mod config;
pub mod prompt;

pub use client::AnthropicLlmClient;
pub use config::{AnthropicConfig, AnthropicConfigError};
