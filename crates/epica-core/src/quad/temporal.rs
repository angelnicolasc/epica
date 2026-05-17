use std::collections::HashMap;

use petgraph::stable_graph::{NodeIndex, StableDiGraph};
use serde::{Deserialize, Serialize};

use crate::belief::BeliefId;

/// Directed temporal relationship between two beliefs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TemporalEdge {
    /// A preceded B: A was established before B.
    /// `gap_ms` is the time between creation timestamps.
    Precedes { gap_ms: u64 },

    /// A and B were established within the same logical step.
    CoOccurs,

    /// A supersedes B: A is a newer version of the same fact.
    Supersedes,
}

/// The temporal projection of the BeliefQuad.
///
/// Tracks precedence, co-occurrence, and supersession between beliefs.
/// The `decay_factor()` computation uses node metadata from the parent quad,
/// not edge data — see [`BeliefQuad::decay_factor`][crate::BeliefQuad::decay_factor].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemporalGraph {
    pub(crate) graph: StableDiGraph<BeliefId, TemporalEdge>,
    pub(crate) indices: HashMap<BeliefId, NodeIndex>,
}

impl TemporalGraph {
    pub fn new() -> Self {
        Self {
            graph: StableDiGraph::new(),
            indices: HashMap::new(),
        }
    }

    pub fn add_node(&mut self, id: BeliefId) -> NodeIndex {
        let idx = self.graph.add_node(id);
        self.indices.insert(id, idx);
        idx
    }

    pub fn remove_node(&mut self, id: BeliefId) {
        if let Some(idx) = self.indices.remove(&id) {
            self.graph.remove_node(idx);
        }
    }

    pub fn add_edge(&mut self, from: BeliefId, to: BeliefId, edge: TemporalEdge) {
        let Some(&fi) = self.indices.get(&from) else { return };
        let Some(&ti) = self.indices.get(&to) else { return };
        self.graph.add_edge(fi, ti, edge);
    }

    /// Compute the exponential decay factor for a belief.
    ///
    /// `decay = exp(-λ · elapsed_ms)` where `λ = 1 / ttl_ms`.
    ///
    /// - If the node has no TTL, returns `1.0` (no decay).
    /// - If `current_ms < created_at_ms` (clock skew), returns `1.0`.
    ///
    /// This is called by `BeliefQuad::decay_factor()` which supplies `created_at_ms`
    /// and `ttl_ms` from the `BeliefNode`.
    pub fn compute_decay(created_at_ms: u64, ttl_ms: Option<u64>, current_ms: u64) -> f32 {
        let Some(ttl) = ttl_ms else { return 1.0 };
        if ttl == 0 {
            return 0.0;
        }
        let elapsed = current_ms.saturating_sub(created_at_ms) as f64;
        let lambda = 1.0 / ttl as f64;
        ((-lambda * elapsed).exp() as f32).clamp(0.0, 1.0)
    }

    /// Returns all beliefs that preceded `id` (incoming `Precedes` edges).
    pub fn predecessors_of(&self, id: BeliefId) -> Vec<BeliefId> {
        use petgraph::{visit::EdgeRef, Direction};
        let Some(&idx) = self.indices.get(&id) else { return vec![] };
        self.graph
            .edges_directed(idx, Direction::Incoming)
            .filter(|e| matches!(e.weight(), TemporalEdge::Precedes { .. }))
            .map(|e| self.graph[e.source()])
            .collect()
    }
}

impl Default for TemporalGraph {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decay_no_ttl_is_one() {
        assert!((TemporalGraph::compute_decay(0, None, 1_000_000) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn decay_at_zero_elapsed_is_one() {
        let now = 1_000_000u64;
        assert!((TemporalGraph::compute_decay(now, Some(60_000), now) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn decay_at_one_ttl_is_exp_neg_one() {
        let d = TemporalGraph::compute_decay(0, Some(1_000), 1_000);
        let expected = std::f64::consts::E.recip() as f32;
        assert!((d - expected).abs() < 1e-5);
    }
}
