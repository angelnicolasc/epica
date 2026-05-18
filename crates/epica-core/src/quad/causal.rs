use std::collections::{HashMap, HashSet};

use petgraph::{
    stable_graph::{NodeIndex, StableDiGraph},
    visit::{Dfs, EdgeRef, Reversed},
    Direction,
};
use serde::{Deserialize, Serialize};

use crate::belief::BeliefId;

/// Directed causal relationship between two beliefs.
///
/// The CausalGraph is the core of Epica's counterfactual and propagation machinery.
/// System 1 propagates `fast_confidence` along `InferredFrom` edges (upstream → downstream).
/// `counterfactual_query()` excludes `descendants_of(antecedent)` from the world view.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CausalEdge {
    /// A directly causes B with a measured effect size and confidence.
    Causes { effect_size: f32, confidence: f32 },

    /// B was inferred from A (and possibly other premises).
    /// `premises` lists all beliefs that jointly entail the target.
    InferredFrom { premises: Vec<BeliefId> },

    /// Counterfactual: "if A had not occurred, B might not exist."
    Counterfactual { antecedent: BeliefId },
}

/// The causal projection of the BeliefQuad.
///
/// Directed acyclic graph (in principle; cycle guards are enforced in traversal
/// since the API cannot prevent user-constructed cycles).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CausalGraph {
    pub(crate) graph: StableDiGraph<BeliefId, CausalEdge>,
    pub(crate) indices: HashMap<BeliefId, NodeIndex>,
}

impl CausalGraph {
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

    pub fn add_edge(&mut self, from: BeliefId, to: BeliefId, edge: CausalEdge) {
        let Some(&fi) = self.indices.get(&from) else { return };
        let Some(&ti) = self.indices.get(&to) else { return };
        self.graph.add_edge(fi, ti, edge);
    }

    /// All beliefs causally downstream of `id` (reachable following outgoing edges).
    ///
    /// Includes transitive descendants. Cycle-safe via a visited set.
    pub fn descendants_of(&self, id: BeliefId) -> HashSet<BeliefId> {
        let Some(&start) = self.indices.get(&id) else { return HashSet::new() };
        let mut dfs = Dfs::new(&self.graph, start);
        let mut result = HashSet::new();
        // Skip start node
        dfs.next(&self.graph);
        while let Some(nx) = dfs.next(&self.graph) {
            result.insert(self.graph[nx]);
        }
        result
    }

    /// All beliefs causally upstream of `id` (reachable following incoming edges).
    ///
    /// Used by `minimal_contraction_set()` to find beliefs that must be retracted
    /// when `id` is revised.
    pub fn ancestors_of(&self, id: BeliefId) -> HashSet<BeliefId> {
        let Some(&start) = self.indices.get(&id) else { return HashSet::new() };
        let reversed = Reversed(&self.graph);
        let mut dfs = Dfs::new(&reversed, start);
        let mut result = HashSet::new();
        dfs.next(&reversed); // skip start
        while let Some(nx) = dfs.next(&reversed) {
            result.insert(self.graph[nx]);
        }
        result
    }

    /// PageRank centrality of `id` in the causal graph.
    ///
    /// Power iteration with damping factor d=0.85, 20 iterations.  Dangling nodes
    /// (zero out-degree) distribute their rank uniformly to all nodes.  The result is
    /// normalised by the number of nodes so the expected value is 1/n, making it
    /// comparable across graphs of different sizes.
    pub fn centrality(&self, id: BeliefId) -> f32 {
        let n = self.indices.len();
        if n == 0 {
            return 0.0;
        }
        const D: f32 = 0.85;
        const ITERS: u32 = 20;
        let uniform = 1.0 / n as f32;
        let mut rank: HashMap<NodeIndex, f32> =
            self.indices.values().map(|&i| (i, uniform)).collect();
        for _ in 0..ITERS {
            let mut next: HashMap<NodeIndex, f32> =
                self.indices.values().map(|&i| (i, (1.0 - D) * uniform)).collect();
            for &src in self.indices.values() {
                let out: Vec<NodeIndex> = self.graph.edges(src).map(|e| e.target()).collect();
                if out.is_empty() {
                    // dangling node: spread rank equally across all nodes
                    let share = D * rank[&src] * uniform;
                    for v in next.values_mut() {
                        *v += share;
                    }
                } else {
                    let share = D * rank[&src] / out.len() as f32;
                    for t in out {
                        *next.get_mut(&t).unwrap() += share;
                    }
                }
            }
            rank = next;
        }
        self.indices.get(&id).and_then(|i| rank.get(i)).copied().unwrap_or(0.0)
    }

    /// Returns all `InferredFrom` premise lists for edges terminating at `id`.
    ///
    /// Used by `propagate_system1()` to collect the confidence sources for Noisy-OR.
    pub fn inferred_from_premises(&self, id: BeliefId) -> Vec<Vec<BeliefId>> {
        let Some(&idx) = self.indices.get(&id) else { return vec![] };
        self.graph
            .edges_directed(idx, Direction::Incoming)
            .filter_map(|e| {
                if let CausalEdge::InferredFrom { premises } = e.weight() {
                    Some(premises.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    /// All incoming causal edges for `id` (any type).
    pub fn incoming_edges(&self, id: BeliefId) -> Vec<(BeliefId, CausalEdge)> {
        use petgraph::visit::EdgeRef;
        let Some(&idx) = self.indices.get(&id) else { return vec![] };
        self.graph
            .edges_directed(idx, Direction::Incoming)
            .map(|e| (self.graph[e.source()], e.weight().clone()))
            .collect()
    }

    /// Iterate over every edge as `(from, to, weight)` tuples.
    ///
    /// Used by the visualisation layer to emit Graphviz DOT and by any
    /// external code that needs to walk the full causal projection without
    /// reaching into `petgraph` internals.
    pub fn all_edges(&self) -> Vec<(BeliefId, BeliefId, CausalEdge)> {
        use petgraph::visit::{EdgeRef, IntoEdgeReferences};
        self.graph
            .edge_references()
            .map(|e| (self.graph[e.source()], self.graph[e.target()], e.weight().clone()))
            .collect()
    }
}

impl Default for CausalGraph {
    fn default() -> Self {
        Self::new()
    }
}
