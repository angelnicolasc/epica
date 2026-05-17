pub mod causal_event;

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::belief::BeliefId;
pub use causal_event::CausalEvent;

/// A single prospective scenario generated at write-time for a belief.
///
/// Kumiho (arXiv:2603.17244): indexing hypothetical future queries at write-time
/// closes the semantic gap between the retrieval cue and the belief's trigger context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProspectiveEntry {
    /// A future query that this belief would help answer.
    pub scenario: String,

    /// Embedding of `scenario` (populated by the LLM embedder in Phase 4).
    /// Empty vec in Phase 1 — `max_prospective_sim()` returns `0.0`.
    pub embedding: Vec<f32>,

    /// Structured causal events extracted from the scenario.
    pub causal_events: Vec<CausalEvent>,
}

/// Write-time prospective index for semantic retrieval.
///
/// **Phase 1 status**: types are complete, but `index_belief()` is a no-op
/// and `max_prospective_sim()` returns `0.0`. Real write-time LLM inference
/// is implemented in Phase 4 (see TD-001 in DEVLOG.md).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProspectiveIndex {
    pub(crate) entries: HashMap<BeliefId, Vec<ProspectiveEntry>>,
}

impl ProspectiveIndex {
    pub fn new() -> Self {
        Self::default()
    }

    /// Invalidate cached prospective entries for `id`.
    ///
    /// Called by `BeliefQuad::revise()` whenever a belief's value changes.
    pub fn invalidate(&mut self, id: BeliefId) {
        self.entries.remove(&id);
    }

    /// Maximum cosine similarity between `query_embedding` and any prospective entry for `id`.
    ///
    /// Phase 1: returns `0.0` (no embeddings stored yet).
    /// Phase 4: computes real cosine similarity over stored scenario embeddings.
    pub fn max_prospective_sim(&self, id: BeliefId, query_embedding: &[f32]) -> f32 {
        let Some(entries) = self.entries.get(&id) else { return 0.0 };
        if query_embedding.is_empty() {
            return 0.0;
        }
        entries
            .iter()
            .filter(|e| !e.embedding.is_empty())
            .map(|e| cosine_similarity(&e.embedding, query_embedding))
            .fold(0.0f32, f32::max)
    }

    /// Returns all entries for `id`, if any.
    pub fn get(&self, id: BeliefId) -> Option<&Vec<ProspectiveEntry>> {
        self.entries.get(&id)
    }

    /// Insert pre-computed entries (used by Phase 4 indexing logic in `BeliefRuntime`).
    pub fn insert(&mut self, id: BeliefId, entries: Vec<ProspectiveEntry>) {
        self.entries.insert(id, entries);
    }
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    (dot / (norm_a * norm_b)).clamp(-1.0, 1.0)
}
