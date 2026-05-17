pub mod noisy_or;

use std::collections::HashSet;

use crate::{belief::BeliefId, quad::BeliefQuad};

/// System 1: fast-path confidence propagation through the CausalGraph.
///
/// This is an **external-runtime approximation** of the UAM described in
/// Agentic UQ (arXiv:2601.15703). The paper's UAM modifies attention weights
/// inside the Transformer — Epica cannot access model internals, so System 1
/// propagates `fast_confidence` through the *external* causal graph instead.
///
/// This approximation is auditable, deterministic, and controllable in ways
/// that the Transformer's internal mechanism is not.
///
/// ## Algorithm per node
///
/// 1. Collect all `InferredFrom` premise lists for edges terminating at `changed`.
/// 2. For each premise list, compute the Noisy-OR combination of premise confidences.
/// 3. Take the maximum across all premise lists (best-evidence rule).
/// 4. Multiply by `decay_factor(changed, now_ms)` from the TemporalGraph.
/// 5. Write to `fast_confidence`.
/// 6. Recurse into causal descendants (with cycle guard).
impl BeliefQuad {
    /// Propagate confidence changes from `changed` downstream through the CausalGraph.
    ///
    /// Always runs synchronously. O(descendants) amortized.
    /// Never activates System 2 — that is the responsibility of `BeliefRuntime` (Phase 2).
    pub fn propagate_system1(&mut self, changed: BeliefId) {
        let mut visited = HashSet::new();
        self.propagate_system1_inner(changed, &mut visited);
    }

    fn propagate_system1_inner(&mut self, id: BeliefId, visited: &mut HashSet<BeliefId>) {
        if !visited.insert(id) {
            return; // cycle guard
        }

        let now_ms = BeliefQuad::now_ms();

        // Collect all premise confidence lists for this node
        let premise_lists = self.causal.inferred_from_premises(id);

        if !premise_lists.is_empty() {
            // For each InferredFrom edge, compute Noisy-OR of that edge's premises
            let per_edge_confidences: Vec<f32> = premise_lists
                .iter()
                .map(|premises| {
                    let confs: Vec<f32> = premises
                        .iter()
                        .filter_map(|&p| self.nodes.get(p).map(|n| n.fast_confidence))
                        .collect();
                    noisy_or::combine(&confs)
                })
                .collect();

            // Best-evidence rule: highest confidence across all InferredFrom edges
            let combined = per_edge_confidences.into_iter().fold(0.0f32, f32::max);

            // Temporal decay
            let decay = self.decay_factor(id, now_ms);
            let new_confidence = (combined * decay).clamp(0.0, 1.0);

            if let Some(node) = self.nodes.get_mut(id) {
                node.fast_confidence = new_confidence;
            }
        }

        // Recurse into causal descendants
        let descendants: Vec<BeliefId> = self.causal.descendants_of(id).into_iter().collect();
        for d in descendants {
            self.propagate_system1_inner(d, visited);
        }
    }
}
