use crate::{
    belief::BeliefId,
    quad::{CausalEdge, EntityEdge, SemanticEdge, TemporalEdge},
};

/// Added and removed edges within one orthogonal graph between two quad states.
#[derive(Debug, Clone)]
pub struct EdgeDiff<E> {
    pub added: Vec<(BeliefId, BeliefId, E)>,
    pub removed: Vec<(BeliefId, BeliefId, E)>,
}

impl<E> Default for EdgeDiff<E> {
    fn default() -> Self {
        Self { added: Vec::new(), removed: Vec::new() }
    }
}

impl<E> EdgeDiff<E> {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty()
    }
}

/// Type aliases for each graph's edge diff.
pub type SemanticEdgeDiff = EdgeDiff<SemanticEdge>;
pub type TemporalEdgeDiff = EdgeDiff<TemporalEdge>;
pub type CausalEdgeDiff = EdgeDiff<CausalEdge>;
pub type EntityEdgeDiff = EdgeDiff<EntityEdge>;
