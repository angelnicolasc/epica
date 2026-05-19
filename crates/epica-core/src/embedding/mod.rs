//! Pluggable embedding provider for semantic equivalence checks (K\*6 / TD-003).
//!
//! ## Why this exists
//!
//! AGM K\*6 (extensionality) requires that *logically equivalent* propositions
//! yield equivalent revised belief sets. Until now Epica approximated K\*6 as
//! `true` (Phase 1 stub) because cross-belief semantic equivalence cannot be
//! decided from literal-string equality. The classic example:
//!
//! ```text
//! current = Asserted("the user wants to refactor authentication")
//! incoming = Asserted("user intent: refactor the auth subsystem")
//! ```
//!
//! Literally distinct, semantically the same. Without an embedding signal the
//! runtime treats the incoming value as a contradiction and runs contraction
//! it should not run.
//!
//! ## Design
//!
//! [`EmbeddingProvider`] is **sync on the hot path**. The actual embedding
//! computation (LLM call, ONNX inference, etc.) is expected to happen in a
//! background warmer or at insert time; the hot path only reads a cache.
//! `embed_cached` returns [`None`] when the text has not been seen — in that
//! case `BeliefQuad` falls back to the literal comparison, preserving the
//! pre-existing behavior with zero risk of regression.
//!
//! No external dependency is pulled in by this module. Concrete providers
//! (Anthropic embeddings, `fastembed-rs`, OpenAI) live in sibling crates.

use std::collections::HashMap;
use std::sync::RwLock;

/// Strategy for deciding semantic equivalence / contradiction from embeddings.
///
/// Implementations MUST be [`Send`] + [`Sync`] because `BeliefQuad` holds an
/// `Arc<dyn EmbeddingProvider>` that crosses thread boundaries (runtime
/// background tasks, MCP handler tasks).
pub trait EmbeddingProvider: Send + Sync {
    /// Return the cached embedding for `text`, or [`None`] if the cache is cold.
    ///
    /// This call is on the AGM hot path. It MUST NOT block on I/O. It MUST NOT
    /// allocate beyond what `Vec::clone` already costs. Implementations that
    /// need to compute a fresh embedding should return [`None`] here and
    /// schedule the work for [`warm`](EmbeddingProvider::warm).
    fn embed_cached(&self, text: &str) -> Option<Vec<f32>>;

    /// Hint the provider to pre-compute embeddings for `texts`.
    ///
    /// The default implementation is a no-op. Cache-backed providers should
    /// override this to spawn the async work that ultimately populates the
    /// cache. Callers MUST NOT depend on the embeddings being available
    /// immediately after `warm` returns.
    fn warm(&self, _texts: &[&str]) {}
}

/// No-op provider: every cache lookup misses, every warm is a no-op.
///
/// This is the default for `BeliefQuad::embedding_provider`. With this provider
/// installed the runtime behaves exactly as it did before the K\*6 semantic
/// upgrade — useful as a baseline in tests and as the safe fallback when no
/// embedding backend is configured.
#[derive(Debug, Default, Clone, Copy)]
pub struct NullEmbeddingProvider;

impl EmbeddingProvider for NullEmbeddingProvider {
    fn embed_cached(&self, _text: &str) -> Option<Vec<f32>> {
        None
    }
}

/// In-memory cache wrapper around a backend `EmbeddingProvider`.
///
/// `CachedEmbeddingProvider` is the common case: a slow upstream (HTTP API,
/// ONNX session, …) is wrapped so that the hot path is a `RwLock` read on a
/// `HashMap<String, Vec<f32>>`. The wrapped backend is consulted on miss only
/// from [`warm`](CachedEmbeddingProvider::warm) — never from
/// [`embed_cached`](EmbeddingProvider::embed_cached), which stays purely
/// in-memory.
///
/// The cache is unbounded in this Sprint 1 implementation. A future revision
/// will swap the `HashMap` for an LRU to bound memory in long-running agents.
pub struct CachedEmbeddingProvider<B: EmbeddingProvider> {
    backend: B,
    cache: RwLock<HashMap<String, Vec<f32>>>,
}

impl<B: EmbeddingProvider> CachedEmbeddingProvider<B> {
    /// Wrap `backend` with an empty cache.
    pub fn new(backend: B) -> Self {
        Self { backend, cache: RwLock::new(HashMap::new()) }
    }

    /// Number of entries currently cached. Test/diagnostic helper.
    pub fn cached_len(&self) -> usize {
        self.cache.read().map(|m| m.len()).unwrap_or(0)
    }

    /// Insert an embedding directly into the cache (bypasses the backend).
    ///
    /// Primarily for tests and for callers that already obtained the vector
    /// out-of-band (e.g., LLM batch endpoints that returned a full table).
    pub fn insert(&self, text: impl Into<String>, embedding: Vec<f32>) {
        if let Ok(mut guard) = self.cache.write() {
            guard.insert(text.into(), embedding);
        }
    }
}

impl<B: EmbeddingProvider> EmbeddingProvider for CachedEmbeddingProvider<B> {
    fn embed_cached(&self, text: &str) -> Option<Vec<f32>> {
        self.cache.read().ok()?.get(text).cloned()
    }

    fn warm(&self, texts: &[&str]) {
        for t in texts {
            // Already cached? Skip.
            if self.cache.read().ok().map(|m| m.contains_key(*t)).unwrap_or(false) {
                continue;
            }
            // Ask the backend's cache; if the backend itself has it, lift it up.
            if let Some(vec) = self.backend.embed_cached(t) {
                if let Ok(mut guard) = self.cache.write() {
                    guard.insert((*t).to_string(), vec);
                }
            } else {
                // Backend didn't have it either. Delegate the warm — the
                // backend is expected to populate its own cache; on a future
                // call we'll lift the value.
                self.backend.warm(&[*t]);
            }
        }
    }
}

impl<B: EmbeddingProvider + std::fmt::Debug> std::fmt::Debug for CachedEmbeddingProvider<B> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CachedEmbeddingProvider")
            .field("backend", &self.backend)
            .field("cached_len", &self.cached_len())
            .finish()
    }
}

/// Cosine similarity between two equal-length f32 vectors.
///
/// Returns `0.0` if either vector has zero norm or if the lengths mismatch.
/// The return value lies in `[-1.0, 1.0]`; equivalence thresholds in
/// [`SemanticEquivalence`] expect values in that range.
#[must_use]
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0_f32;
    let mut na = 0.0_f32;
    let mut nb = 0.0_f32;
    for i in 0..a.len() {
        let x = a[i];
        let y = b[i];
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    let denom = (na.sqrt()) * (nb.sqrt());
    if denom == 0.0 { 0.0 } else { dot / denom }
}

/// Thresholds for cosine-similarity-based equivalence and contradiction.
///
/// A pair of texts with cosine similarity `s` is classified as:
/// - `s >= equivalence`: semantically equivalent (no contradiction even if
///   the strings differ literally — the K\*6 case);
/// - `s <= contradiction`: actively opposed (treated as contradiction
///   regardless of literal match);
/// - otherwise: undecided, fall back to literal comparison.
#[derive(Debug, Clone, Copy)]
pub struct SemanticEquivalence {
    /// Cosine similarity at-or-above which two texts are considered equivalent.
    /// Default: `0.92` (empirically the cutoff between paraphrase and
    /// "related but distinct" for most modern sentence-embedding models).
    pub equivalence: f32,
    /// Cosine similarity at-or-below which two texts are considered to
    /// contradict each other. Default: `-0.30` (well-separated in
    /// embedding space).
    pub contradiction: f32,
}

impl Default for SemanticEquivalence {
    fn default() -> Self {
        Self { equivalence: 0.92, contradiction: -0.30 }
    }
}

impl SemanticEquivalence {
    /// Classify `(a, b)` given their embeddings.
    #[must_use]
    pub fn classify(&self, a: &[f32], b: &[f32]) -> EquivalenceVerdict {
        let s = cosine_similarity(a, b);
        if s >= self.equivalence {
            EquivalenceVerdict::Equivalent(s)
        } else if s <= self.contradiction {
            EquivalenceVerdict::Contradicts(s)
        } else {
            EquivalenceVerdict::Undecided(s)
        }
    }
}

/// Outcome of a semantic-equivalence comparison.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EquivalenceVerdict {
    /// Texts are paraphrases / synonymous; cosine `>= equivalence`.
    Equivalent(f32),
    /// Texts are actively opposed; cosine `<= contradiction`.
    Contradicts(f32),
    /// Cosine fell in the undecided band; caller should fall back to literal
    /// comparison.
    Undecided(f32),
}

impl EquivalenceVerdict {
    /// Raw cosine similarity value behind the verdict.
    pub fn score(self) -> f32 {
        match self {
            Self::Equivalent(s) | Self::Contradicts(s) | Self::Undecided(s) => s,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_provider_always_misses() {
        let p = NullEmbeddingProvider;
        assert!(p.embed_cached("hello").is_none());
        p.warm(&["hello"]);
        assert!(p.embed_cached("hello").is_none());
    }

    #[test]
    fn cached_provider_round_trip() {
        let p = CachedEmbeddingProvider::new(NullEmbeddingProvider);
        assert_eq!(p.cached_len(), 0);
        p.insert("foo", vec![1.0, 0.0, 0.0]);
        assert_eq!(p.cached_len(), 1);
        assert_eq!(p.embed_cached("foo"), Some(vec![1.0, 0.0, 0.0]));
        assert!(p.embed_cached("bar").is_none());
    }

    #[test]
    fn cosine_known_cases() {
        // Identical vectors: 1.0
        let v = vec![1.0, 2.0, 3.0];
        assert!((cosine_similarity(&v, &v) - 1.0).abs() < 1e-6);
        // Orthogonal: 0.0
        assert!(cosine_similarity(&[1.0, 0.0], &[0.0, 1.0]).abs() < 1e-6);
        // Anti-parallel: -1.0
        assert!((cosine_similarity(&[1.0, 0.0], &[-1.0, 0.0]) + 1.0).abs() < 1e-6);
        // Length mismatch: 0.0
        assert_eq!(cosine_similarity(&[1.0, 0.0], &[1.0, 0.0, 0.0]), 0.0);
        // Empty: 0.0
        assert_eq!(cosine_similarity(&[], &[]), 0.0);
        // Zero-norm: 0.0
        assert_eq!(cosine_similarity(&[0.0, 0.0], &[1.0, 1.0]), 0.0);
    }

    #[test]
    fn equivalence_classify_bands() {
        let eq = SemanticEquivalence::default();
        // Identical → Equivalent
        let a = vec![1.0_f32, 0.0, 0.0];
        assert!(matches!(eq.classify(&a, &a), EquivalenceVerdict::Equivalent(_)));
        // Anti-parallel → Contradicts
        let b = vec![-1.0_f32, 0.0, 0.0];
        assert!(matches!(eq.classify(&a, &b), EquivalenceVerdict::Contradicts(_)));
        // Orthogonal → Undecided
        let c = vec![0.0_f32, 1.0, 0.0];
        assert!(matches!(eq.classify(&a, &c), EquivalenceVerdict::Undecided(_)));
    }
}
