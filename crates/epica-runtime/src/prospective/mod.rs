//! Write-time prospective indexing traits and offline fallback embedder.
//!
//! Implements the Kumiho (arXiv:2603.17244) insight: closing the semantic gap
//! between the retrieval cue and the belief's write-time context by indexing
//! hypothetical future queries at insert time, not at retrieval time.
//!
//! ## Components
//!
//! - [`Embedder`] — async trait for text → `Vec<f32>` dense embedding.
//! - [`ProspectiveClient`] — async trait for LLM-based future scenario generation.
//! - [`HashEmbedder`] — deterministic word-hash embedder; zero network deps,
//!   suitable for unit tests and offline environments.
//! - [`RawProspectiveScenario`] — one scenario as returned by the LLM client.

use epica_core::CausalEvent;
use serde::{Deserialize, Serialize};

// ── Error types ───────────────────────────────────────────────────────────────

/// Error returned by [`Embedder::embed`].
#[derive(Debug, thiserror::Error)]
pub enum EmbedError {
    #[error("embedding model error: {0}")]
    Model(String),
    #[error("network error: {0}")]
    Network(String),
}

/// Error returned by [`ProspectiveClient::generate_scenarios`].
#[derive(Debug, thiserror::Error)]
pub enum ProspectiveClientError {
    #[error("network error: {0}")]
    Network(String),
    #[error("deserialization failed: {0}")]
    Deserialize(String),
    #[error("rate limited")]
    RateLimited,
}

// ── Data types ────────────────────────────────────────────────────────────────

/// One prospective scenario as returned by the LLM client before embedding.
///
/// The `embedding` field is populated by [`Embedder::embed`] in
/// `BeliefRuntime::index_belief_prospective()` before the scenario is stored.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawProspectiveScenario {
    /// A natural-language future query this belief would help answer.
    pub scenario: String,
    /// Structured causal events extracted from the scenario.
    /// Populated by the LLM client; may be empty in Phase 4.
    pub causal_events: Vec<CausalEvent>,
}

// ── Traits ────────────────────────────────────────────────────────────────────

/// Async trait for converting text into a dense embedding vector.
///
/// Implement this trait to plug in any embedding backend:
/// - [`HashEmbedder`] — offline, deterministic, no network calls.
/// - An Anthropic or OpenAI embeddings client for production deployments.
#[async_trait::async_trait]
pub trait Embedder: Send + Sync {
    /// Embed `text` into a dense `Vec<f32>`.
    ///
    /// The returned vector should be L2-normalised so that cosine similarity
    /// equals the dot product — [`ProspectiveIndex::max_prospective_sim`] assumes this.
    async fn embed(&self, text: &str) -> Result<Vec<f32>, EmbedError>;
}

/// Async trait for LLM-based prospective scenario generation.
///
/// Given a belief key and its value representation, returns a list of
/// future queries this belief would help answer.  Implement for your LLM
/// backend; `epica-anthropic` provides `AnthropicLlmClient: ProspectiveClient`.
#[async_trait::async_trait]
pub trait ProspectiveClient: Send + Sync {
    /// Generate a list of future query scenarios for the given belief.
    ///
    /// `belief_key` — the belief's string key (e.g. `"user_intent"`).
    /// `belief_value_repr` — `Debug` representation of the `BeliefValue`.
    async fn generate_scenarios(
        &self,
        belief_key: &str,
        belief_value_repr: &str,
    ) -> Result<Vec<RawProspectiveScenario>, ProspectiveClientError>;
}

// ── HashEmbedder ──────────────────────────────────────────────────────────────

/// Deterministic word-hash embedder.
///
/// Maps each whitespace-delimited word to a bucket in a `dim`-dimensional
/// vector via FNV-1a, then L2-normalises. Requires no network calls or
/// external models — suitable for unit tests, CI, and offline environments.
///
/// Cosine similarity between two `HashEmbedder` vectors is non-zero iff the
/// two texts share at least one word (case-insensitive), making retrieval
/// ranking observable in tests without a real embedding service.
pub struct HashEmbedder {
    pub dim: usize,
}

impl HashEmbedder {
    pub fn new(dim: usize) -> Self {
        assert!(dim > 0, "embedding dimension must be > 0");
        Self { dim }
    }
}

#[async_trait::async_trait]
impl Embedder for HashEmbedder {
    async fn embed(&self, text: &str) -> Result<Vec<f32>, EmbedError> {
        let mut v = vec![0.0f32; self.dim];
        for word in text.split_whitespace() {
            let bucket = fnv1a(word.to_lowercase().as_bytes()) as usize % self.dim;
            v[bucket] += 1.0;
        }
        l2_normalize(&mut v);
        Ok(v)
    }
}

// ── Internal helpers ──────────────────────────────────────────────────────────

fn fnv1a(bytes: &[u8]) -> u64 {
    const OFFSET: u64 = 14_695_981_039_346_656_037;
    const PRIME: u64 = 1_099_511_628_211;
    bytes.iter().fold(OFFSET, |h, &b| (h ^ b as u64).wrapping_mul(PRIME))
}

fn l2_normalize(v: &mut [f32]) {
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        v.iter_mut().for_each(|x| *x /= norm);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn hash_embedder_returns_correct_dim() {
        let e = HashEmbedder::new(128);
        let v = e.embed("hello world").await.unwrap();
        assert_eq!(v.len(), 128);
    }

    #[tokio::test]
    async fn hash_embedder_is_normalised() {
        let e = HashEmbedder::new(64);
        let v = e.embed("belief revision agm").await.unwrap();
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-5, "norm={norm}");
    }

    #[tokio::test]
    async fn identical_texts_have_similarity_one() {
        let e = HashEmbedder::new(64);
        let a = e.embed("user intent refactor auth").await.unwrap();
        let b = e.embed("user intent refactor auth").await.unwrap();
        let sim: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        assert!((sim - 1.0).abs() < 1e-5);
    }

    #[tokio::test]
    async fn disjoint_texts_have_zero_similarity() {
        let e = HashEmbedder::new(256);
        // Use unique words unlikely to hash to the same bucket
        let a = e.embed("zyzzyva quetzal").await.unwrap();
        let b = e.embed("flibbertigibbet widdershins").await.unwrap();
        let sim: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        // With large dim, probability of collision is negligible
        assert!(sim.abs() < 0.1, "sim={sim}");
    }
}
