use std::collections::HashMap;

use petgraph::{
    stable_graph::{NodeIndex, StableDiGraph},
    visit::EdgeRef,
    Direction,
};
use serde::{Deserialize, Serialize};

use crate::belief::{BeliefId, BeliefValue};

/// Directed semantic relationship between two beliefs.
///
/// MAGMA (arXiv:2601.03236) demonstrates that mixing semantic, temporal, causal, and
/// entity edges in a single graph makes retrieval ambiguous by design. The `SemanticGraph`
/// isolates concept similarity and subsumption relationships.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SemanticEdge {
    /// A subsumes B: A is a more general concept than B.
    /// Directed: A → B means "A is broader than B".
    Subsumes,

    /// A and B assert contradictory facts about the same subject.
    /// Stored bidirectionally: both A→B and B→A edges are added.
    Contradicts,

    /// A and B are semantically equivalent (synonym or alias).
    /// Stored bidirectionally.
    Synonymous,
}

/// The semantic projection of the BeliefQuad.
///
/// Internally a `petgraph::stable_graph::StableDiGraph` so that node removal does not
/// invalidate `NodeIndex` values for other nodes. The `indices` map provides O(1)
/// lookup from `BeliefId` to `NodeIndex`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticGraph {
    pub(crate) graph: StableDiGraph<BeliefId, SemanticEdge>,
    pub(crate) indices: HashMap<BeliefId, NodeIndex>,
}

impl SemanticGraph {
    pub fn new() -> Self {
        Self {
            graph: StableDiGraph::new(),
            indices: HashMap::new(),
        }
    }

    /// Register a belief in the semantic graph. Must be called whenever a node is
    /// inserted into the parent `BeliefQuad`.
    pub fn add_node(&mut self, id: BeliefId) -> NodeIndex {
        let idx = self.graph.add_node(id);
        self.indices.insert(id, idx);
        idx
    }

    /// Remove a belief from the semantic graph. Also removes all its edges.
    pub fn remove_node(&mut self, id: BeliefId) {
        if let Some(idx) = self.indices.remove(&id) {
            self.graph.remove_node(idx);
        }
    }

    /// Add a directed edge. For symmetric relationships (`Contradicts`, `Synonymous`),
    /// the reverse edge is added automatically.
    pub fn add_edge(&mut self, from: BeliefId, to: BeliefId, edge: SemanticEdge) {
        let Some(&fi) = self.indices.get(&from) else { return };
        let Some(&ti) = self.indices.get(&to) else { return };
        let symmetric = matches!(edge, SemanticEdge::Contradicts | SemanticEdge::Synonymous);
        self.graph.add_edge(fi, ti, edge.clone());
        if symmetric {
            self.graph.add_edge(ti, fi, edge);
        }
    }

    /// Returns `true` if `id` has any outgoing `Contradicts` edge.
    ///
    /// Used by `BeliefQuad::check_contradiction()` as the graph-structural component
    /// of the K*4 vacuity check. Value-level contradiction is checked separately.
    pub fn has_contradiction_edges(&self, id: BeliefId) -> bool {
        let Some(&idx) = self.indices.get(&id) else { return false };
        self.graph
            .edges(idx)
            .any(|e| matches!(e.weight(), SemanticEdge::Contradicts))
    }

    /// Returns beliefs that subsume `id` (incoming `Subsumes` edges).
    pub fn supersets_of(&self, id: BeliefId) -> Vec<BeliefId> {
        let Some(&idx) = self.indices.get(&id) else { return vec![] };
        self.graph
            .edges_directed(idx, Direction::Incoming)
            .filter(|e| matches!(e.weight(), SemanticEdge::Subsumes))
            .map(|e| self.graph[e.source()])
            .collect()
    }

    /// Returns beliefs subsumed by `id` (outgoing `Subsumes` edges).
    pub fn subsets_of(&self, id: BeliefId) -> Vec<BeliefId> {
        let Some(&idx) = self.indices.get(&id) else { return vec![] };
        self.graph
            .edges(idx)
            .filter(|e| matches!(e.weight(), SemanticEdge::Subsumes))
            .map(|e| self.graph[e.target()])
            .collect()
    }

    /// Iterate over every semantic edge as `(from, to, weight)`.
    ///
    /// Used by the visualisation layer to render semantic relationships
    /// alongside the causal graph.
    pub fn all_edges(&self) -> Vec<(BeliefId, BeliefId, SemanticEdge)> {
        use petgraph::visit::IntoEdgeReferences;
        self.graph
            .edge_references()
            .map(|e| (self.graph[e.source()], self.graph[e.target()], e.weight().clone()))
            .collect()
    }

    /// Phase 1 value-level contradiction check.
    ///
    /// Returns `true` if `new_value` directly contradicts the existing value on a
    /// same-ID update (different value → the belief has changed). This detects
    /// structural contradiction on a single belief: not semantic contradiction
    /// between two distinct beliefs.
    ///
    /// **Scope**: same-ID structural comparison only. Cross-belief semantic
    /// contradiction (paraphrases, negations, synonyms across distinct beliefs)
    /// requires explicit `SemanticEdge::Contradicts` edges or Phase 4 embedding
    /// integration (TD-003).
    ///
    /// `Asserted` strings are normalized (lowercase, trimmed) before comparison to
    /// prevent false-positive contradictions when the same fact is restated with
    /// minor formatting differences.
    pub fn value_contradicts(current_value: &BeliefValue, new_value: &BeliefValue) -> bool {
        match (current_value, new_value) {
            (BeliefValue::Deterministic(a), BeliefValue::Deterministic(b)) => a != b,
            (BeliefValue::Inferred(a), BeliefValue::Inferred(b)) => a != b,
            (BeliefValue::Asserted(a), BeliefValue::Asserted(b)) => {
                a.trim().to_lowercase() != b.trim().to_lowercase()
            }
            (BeliefValue::Reference(a), BeliefValue::Reference(b)) => a != b,
            // Cross-variant changes are treated as contradictions
            _ => true,
        }
    }
}

impl Default for SemanticGraph {
    fn default() -> Self {
        Self::new()
    }
}
