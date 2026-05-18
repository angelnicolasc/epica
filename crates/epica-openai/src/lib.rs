//! # epica-openai
//!
//! [`OpenAiLlmClient`]: a production [`LlmClient`] implementation for Epica's
//! System 2 (UAR — Uncertainty-Aware Reflection, arXiv:2601.15703) using the
//! OpenAI Chat Completions API.
//!
//! ## Design
//!
//! System 2 recalibrates a belief's confidence when System 1 diverges
//! significantly from the agent's reliability baseline. This crate sends a
//! structured reflection request via OpenAI's tool / function-calling API
//! with a forced `tool_choice`, guaranteeing the model returns
//! `revised_confidence` and `reasoning` as typed JSON — no text parsing,
//! no ambiguity.
//!
//! The retry policy (exponential backoff with deterministic jitter on 429
//! and 5xx, up to 3 attempts) mirrors `epica-anthropic` so both providers
//! present the same observable behaviour to the runtime.
//!
//! ## Quick start
//!
//! ```rust,no_run
//! use epica_openai::OpenAiLlmClient;
//! use epica_runtime::BeliefRuntime;
//! use epica_core::BeliefQuad;
//! use std::sync::Arc;
//!
//! # async fn run() -> Result<(), Box<dyn std::error::Error>> {
//! // Reads OPENAI_API_KEY from the environment
//! let client = OpenAiLlmClient::from_env()?;
//!
//! let rt = BeliefRuntime::new(BeliefQuad::new(), 0.5, 20, 2.0)
//!     .with_llm_client(Arc::new(client));
//! # Ok(())
//! # }
//! ```
//!
//! [`LlmClient`]: epica_runtime::LlmClient

#![warn(missing_docs)]

pub mod client;
pub mod config;

pub use client::OpenAiLlmClient;
pub use config::{OpenAiConfig, OpenAiConfigError};
