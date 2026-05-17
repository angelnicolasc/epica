pub mod causal;
pub mod entity;
pub mod semantic;
pub mod temporal;

use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use slotmap::SlotMap;

use crate::{
    belief::{BeliefId, BeliefNode, BeliefValue},
    checkpoint::types::BeliefQuadCheckpoint,
    prospective::ProspectiveIndex,
};

pub use causal::CausalEdge;
pub use entity::EntityEdge;
pub use semantic::SemanticEdge;
pub use temporal::TemporalEdge;

use causal::CausalGraph;
use entity::EntityGraph;
use semantic::SemanticGraph;
use temporal::TemporalGraph;

/// The central data structure of Epica.
///
/// A `BeliefQuad` maintains **four orthogonal graphs** synchronized over a shared
/// [`SlotMap`] of [`BeliefNode`]s. Every node exists simultaneously in all four
/// projections; removing a node removes it from all four.
///
/// ## Why four graphs?
///
/// MAGMA (arXiv:2601.03236) demonstrates empirically that a monolithic graph entangles
/// temporal, causal, semantic, and entity information — making retrieval ambiguous by
/// design. The four orthogonal graphs each answer a distinct class of query:
///
/// - **Semantic**: "what beliefs talk about the same concept?"
/// - **Temporal**: "what happened before? what beliefs are stale?"
/// - **Causal**: "why does the agent believe X? what if Y had not occurred?"
/// - **Entity**: "who produced this belief? who can modify it?"
///
/// ## Invariants
///
/// 1. `BeliefId` is the single source of truth. The four graphs store `BeliefId` as
///    node weights, not copies of `BeliefNode`.
/// 2. Each graph maintains a `HashMap<BeliefId, NodeIndex>` for O(1) lookup.
/// 3. `propagate_system1()` always carries a visited set to guard against cycles.
/// 4. `prospective_index` exists from Phase 1 to stabilize serialization formats.
///
/// ## Serialization
///
/// `BeliefQuad` implements `Serialize`/`Deserialize` for persistence via
/// [`LongTermMemoryStore`][crate::LongTermMemoryStore].  In-session `checkpoints` are
/// ephemeral and are skipped during serialization; they are not restored on load.
#[derive(Serialize, Deserialize)]
pub struct BeliefQuad {
    pub(crate) nodes: SlotMap<BeliefId, BeliefNode>,
    pub(crate) semantic: SemanticGraph,
    pub(crate) temporal: TemporalGraph,
    pub(crate) causal: CausalGraph,
    pub(crate) entity: EntityGraph,
    pub(crate) version: u64,
    /// In-session checkpoints — not persisted.
    #[serde(skip)]
    pub(crate) checkpoints: Vec<BeliefQuadCheckpoint>,
    pub(crate) prospective_index: ProspectiveIndex,
}

// Manual Clone: derive would work too but we want to be explicit about the cost.
impl Clone for BeliefQuad {
    fn clone(&self) -> Self {
        Self {
            nodes: self.nodes.clone(),
            semantic: self.semantic.clone(),
            temporal: self.temporal.clone(),
            causal: self.causal.clone(),
            entity: self.entity.clone(),
            version: self.version,
            checkpoints: self.checkpoints.clone(),
            prospective_index: self.prospective_index.clone(),
        }
    }
}

impl std::fmt::Debug for BeliefQuad {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BeliefQuad")
            .field("nodes", &self.nodes.len())
            .field("version", &self.version)
            .field("checkpoints", &self.checkpoints.len())
            .finish()
    }
}

impl BeliefQuad {
    /// Create an empty quad.
    pub fn new() -> Self {
        Self {
            nodes: SlotMap::with_key(),
            semantic: SemanticGraph::new(),
            temporal: TemporalGraph::new(),
            causal: CausalGraph::new(),
            entity: EntityGraph::new(),
            version: 0,
            checkpoints: Vec::new(),
            prospective_index: ProspectiveIndex::new(),
        }
    }

    // ── Core CRUD ────────────────────────────────────────────────────────────

    /// Insert a belief into the quad. Returns the assigned [`BeliefId`].
    ///
    /// The node is registered in all four graphs immediately. Any edges connecting
    /// it to existing beliefs must be added separately via `add_*_edge()`.
    pub fn insert(&mut self, mut node: BeliefNode) -> BeliefId {
        let id = self.nodes.insert_with_key(|k| {
            node.id = k;
            node
        });
        self.semantic.add_node(id);
        self.temporal.add_node(id);
        self.causal.add_node(id);
        self.entity.add_node(id);
        self.version += 1;
        id
    }

    /// Get an immutable reference to a belief by ID.
    #[inline]
    pub fn get(&self, id: BeliefId) -> Option<&BeliefNode> {
        self.nodes.get(id)
    }

    /// Get a mutable reference to a belief by ID.
    #[inline]
    pub fn get_mut(&mut self, id: BeliefId) -> Option<&mut BeliefNode> {
        self.nodes.get_mut(id)
    }

    /// Remove a belief from the quad and all four graphs. Returns the removed node.
    pub fn remove(&mut self, id: BeliefId) -> Option<BeliefNode> {
        let node = self.nodes.remove(id)?;
        self.semantic.remove_node(id);
        self.temporal.remove_node(id);
        self.causal.remove_node(id);
        self.entity.remove_node(id);
        self.prospective_index.invalidate(id);
        self.version += 1;
        Some(node)
    }

    /// Iterate over all (id, node) pairs.
    #[inline]
    pub fn iter(&self) -> slotmap::basic::Iter<'_, BeliefId, BeliefNode> {
        self.nodes.iter()
    }

    /// Number of beliefs in the quad.
    #[inline]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Returns `true` if the quad contains no beliefs.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Current version counter. Incremented on every structural change.
    #[inline]
    pub fn version(&self) -> u64 {
        self.version
    }

    // ── Edge management ───────────────────────────────────────────────────────

    /// Add a semantic relationship between two beliefs.
    pub fn add_semantic_edge(&mut self, from: BeliefId, to: BeliefId, edge: SemanticEdge) {
        self.semantic.add_edge(from, to, edge);
    }

    /// Add a temporal relationship between two beliefs.
    pub fn add_temporal_edge(&mut self, from: BeliefId, to: BeliefId, edge: TemporalEdge) {
        self.temporal.add_edge(from, to, edge);
    }

    /// Add a causal relationship between two beliefs.
    pub fn add_causal_edge(&mut self, from: BeliefId, to: BeliefId, edge: CausalEdge) {
        self.causal.add_edge(from, to, edge);
    }

    /// Add an entity provenance relationship between two beliefs.
    pub fn add_entity_edge(&mut self, from: BeliefId, to: BeliefId, edge: EntityEdge) {
        self.entity.add_edge(from, to, edge);
    }

    // ── Temporal utilities ────────────────────────────────────────────────────

    /// Exponential decay factor for a belief at `current_ms`.
    ///
    /// Delegates to [`TemporalGraph::compute_decay`] using the node's
    /// `created_at_ms` and `ttl_ms`. Returns `1.0` if the node is not found.
    pub fn decay_factor(&self, id: BeliefId, current_ms: u64) -> f32 {
        match self.nodes.get(id) {
            None => 1.0,
            Some(n) => TemporalGraph::compute_decay(n.created_at_ms, n.ttl_ms, current_ms),
        }
    }

    /// Current wall-clock time as milliseconds since UNIX_EPOCH.
    pub fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    // ── Contradiction detection (used by AGM revision) ────────────────────────

    /// Returns `true` if setting belief `id` to `new_value` would create a contradiction
    /// in the current state of the quad.
    ///
    /// Phase 1: checks value-level equality AND explicit `SemanticEdge::Contradicts` edges.
    /// Phase 4 (TD-003): embedding-based semantic contradiction detection.
    pub(crate) fn check_contradiction(&self, id: BeliefId, new_value: &BeliefValue) -> bool {
        let Some(node) = self.nodes.get(id) else { return false };
        SemanticGraph::value_contradicts(&node.value, new_value)
            || self.semantic.has_contradiction_edges(id)
    }

    // ── Read-only graph accessors (used by system1, diff, counterfactual) ─────

    pub fn causal(&self) -> &CausalGraph {
        &self.causal
    }

    pub fn prospective_index_mut(&mut self) -> &mut ProspectiveIndex {
        &mut self.prospective_index
    }

    pub fn prospective_index(&self) -> &ProspectiveIndex {
        &self.prospective_index
    }
}

impl Default for BeliefQuad {
    fn default() -> Self {
        Self::new()
    }
}
