use slotmap::new_key_type;

new_key_type! {
    /// Stable, arena-allocated identifier for a [`BeliefNode`][crate::BeliefNode].
    ///
    /// Derived from `slotmap` — O(1) lookup, no ABA hazard, never reuses a live slot.
    /// All four orthogonal graphs in the [`BeliefQuad`][crate::BeliefQuad] index nodes
    /// via this key, not by copying the node data.
    pub struct BeliefId;
}
