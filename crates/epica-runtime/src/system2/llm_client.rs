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
#[derive(Debug, thiserror::Error)]
pub enum LlmClientError {
    #[error("network error: {0}")]
    Network(String),

    #[error("deserialization failed: {0}")]
    Deserialize(String),

    #[error("rate limited")]
    RateLimited,
}
