use std::collections::HashMap;

use petgraph::{stable_graph::{NodeIndex, StableDiGraph}, Direction};
use serde::{Deserialize, Serialize};

use crate::belief::BeliefId;

/// Provenance relationship between a belief and an agent or tool.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum EntityEdge {
    /// This belief was produced by the given agent/tool.
    ProducedBy { agent_id: String },

    /// This belief was consumed (read and acted upon) by an agent.
    Consumed { by_agent: String, at_turn: u32 },

    /// This belief was shared with multiple downstream agents.
    SharedWith { agents: Vec<String> },
}

/// The entity provenance projection of the BeliefQuad.
///
/// Tracks which agent or tool produced, consumed, or propagated each belief.
/// Supports the `write_auth` and `read_auth` enforcement in `MnemonicSovereignty` (Phase 3).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityGraph {
    pub(crate) graph: StableDiGraph<BeliefId, EntityEdge>,
    pub(crate) indices: HashMap<BeliefId, NodeIndex>,
}

impl EntityGraph {
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

    pub fn add_edge(&mut self, from: BeliefId, to: BeliefId, edge: EntityEdge) {
        let Some(&fi) = self.indices.get(&from) else { return };
        let Some(&ti) = self.indices.get(&to) else { return };
        self.graph.add_edge(fi, ti, edge);
    }

    /// Returns the agent IDs that produced `id`.
    pub fn producers_of(&self, id: BeliefId) -> Vec<String> {
        let Some(&idx) = self.indices.get(&id) else { return vec![] };
        self.graph
            .edges_directed(idx, Direction::Incoming)
            .filter_map(|e| {
                if let EntityEdge::ProducedBy { agent_id } = e.weight() {
                    Some(agent_id.clone())
                } else {
                    None
                }
            })
            .collect()
    }
}

impl Default for EntityGraph {
    fn default() -> Self {
        Self::new()
    }
}
