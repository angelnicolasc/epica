//! LLM client trait for System 2 reflection calls.
//!
//! Phase 2: real implementation.

use serde::{Deserialize, Serialize};

/// Diagnostic signal computed by System 1 and passed to System 2.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticSignal {
    pub belief_key: String,
    pub fast_confidence: f32,
    pub reliability_baseline: f32,
    pub divergence: f32,
}

/// The result of System 2 inverse optimization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct System2Result {
    pub revised_confidence: f32,
    pub reasoning: String,
}

/// Trait for LLM clients used by System 2 reflection.
///
/// Phase 2: implement for Anthropic SDK, OpenAI, or any HTTP-based LLM.
/// `complete_json` is intentionally NOT on this trait to preserve dyn compatibility.
/// Callers that need structured completion can add it on their concrete type.
#[cfg(feature = "system2")]
#[async_trait::async_trait]
pub trait LlmClient: Send + Sync {
    async fn reflect(
        &self,
        diagnostic: &DiagnosticSignal,
    ) -> Result<System2Result, LlmClientError>;
}

/// Errors from LLM client calls.
///
/// Variants are split along the retry-policy axis: implementations classify
/// each failure mode by whether retrying it can plausibly succeed, so the
/// retry layer can be a one-liner over `is_retryable`.
#[derive(Debug, thiserror::Error)]
pub enum LlmClientError {
    /// Network / transport failure or 5xx response. Retryable.
    #[error("network error: {0}")]
    Network(String),

    /// HTTP 4xx other than 429 — bad request, auth, payload-too-large, etc.
    /// Not retryable: the same call will fail the same way on the next attempt.
    #[error("client error: {0}")]
    ClientError(String),

    /// Response could not be parsed into the expected schema. Not retryable.
    #[error("deserialization failed: {0}")]
    Deserialize(String),

    /// HTTP 429 — retryable after backoff.
    #[error("rate limited")]
    RateLimited,
}
