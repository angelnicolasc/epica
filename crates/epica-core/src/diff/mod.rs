pub mod edge_diff;
pub mod tece;

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::belief::{BeliefId, BeliefNode, BeliefValue};
use crate::quad::BeliefQuad;
use edge_diff::{CausalEdgeDiff, EntityEdgeDiff, SemanticEdgeDiff, TemporalEdgeDiff};

pub use edge_diff::EdgeDiff;

/// Serializable snapshot of a single belief at a point in time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BeliefSnapshot {
    pub id: BeliefId,
    pub key: String,
    pub value: BeliefValue,
    pub fast_confidence: f32,
    pub slow_confidence: Option<f32>,
    pub version_at_snapshot: u64,
}

impl BeliefSnapshot {
    fn from_node(node: &BeliefNode, version: u64) -> Self {
        Self {
            id: node.id,
            key: node.key.clone(),
            value: node.value.clone(),
            fast_confidence: node.fast_confidence,
            slow_confidence: node.slow_confidence,
            version_at_snapshot: version,
        }
    }
}

/// A field-level change to a belief.
#[derive(Debug, Clone)]
pub struct BeliefModification {
    pub id: BeliefId,
    pub key: String,
    pub old_value: BeliefValue,
    pub new_value: BeliefValue,
    pub old_confidence: f32,
    pub new_confidence: f32,
}

/// Structural diff between two `BeliefQuad` states.
///
/// The canonical root-cause report — pure diff of data structures,
/// no LLM-as-judge, no heuristics.
#[derive(Debug, Clone, Default)]
pub struct BeliefQuadDiff {
    pub added: Vec<BeliefSnapshot>,
    pub removed: Vec<BeliefSnapshot>,
    pub modified: Vec<BeliefModification>,

    pub semantic_edges: SemanticEdgeDiff,
    pub temporal_edges: TemporalEdgeDiff,
    pub causal_edges: CausalEdgeDiff,
    pub entity_edges: EntityEdgeDiff,

    /// Mean confidence delta per belief key: (old_confidence, new_confidence).
    pub confidence_delta: HashMap<String, (f32, f32)>,

    /// Trajectory-ECE for this interval. `None` when no outcome history is available.
    /// Populated by `BeliefRuntime` (Phase 2) which tracks per-step outcomes.
    pub trajectory_ece: Option<f32>,
}

impl BeliefQuadDiff {
    /// Returns `true` if any beliefs were added, removed, or modified.
    ///
    /// Used by `rollback_to()` to detect whether the current state differs from the
    /// checkpoint. If nothing changed, rollback is a no-op and is rejected (K*4 vacuity).
    /// Added beliefs count because rolling them back requires contraction.
    pub fn has_contradictions(&self) -> bool {
        !self.added.is_empty() || !self.modified.is_empty() || !self.removed.is_empty()
    }

    pub fn is_empty(&self) -> bool {
        self.added.is_empty()
            && self.removed.is_empty()
            && self.modified.is_empty()
            && self.semantic_edges.is_empty()
            && self.temporal_edges.is_empty()
            && self.causal_edges.is_empty()
            && self.entity_edges.is_empty()
    }
}

/// Diff implementation on `BeliefQuad`.
impl BeliefQuad {
    /// Compute the structural diff from `other` to `self`.
    ///
    /// Added: in `self` but not in `other` (by belief key).
    /// Removed: in `other` but not in `self` (by belief key).
    /// Modified: key exists in both but value or confidence changed.
    ///
    /// O(n) via HashMap lookup by key. No LLM calls.
    pub fn diff(&self, other: &BeliefQuad) -> BeliefQuadDiff {
        let self_by_key: HashMap<&str, &BeliefNode> =
            self.nodes.values().map(|n| (n.key.as_str(), n)).collect();
        let other_by_key: HashMap<&str, &BeliefNode> =
            other.nodes.values().map(|n| (n.key.as_str(), n)).collect();

        let mut added = Vec::new();
        let mut removed = Vec::new();
        let mut modified = Vec::new();
        let mut confidence_delta = HashMap::new();

        // Beliefs in self but not other → added
        for (key, self_node) in &self_by_key {
            if !other_by_key.contains_key(key) {
                added.push(BeliefSnapshot::from_node(self_node, self.version));
            }
        }

        // Beliefs in other but not self → removed
        for (key, other_node) in &other_by_key {
            if !self_by_key.contains_key(key) {
                removed.push(BeliefSnapshot::from_node(other_node, other.version));
            }
        }

        // Beliefs in both → check for modifications
        for (key, self_node) in &self_by_key {
            if let Some(other_node) = other_by_key.get(key) {
                let value_changed = self_node.value != other_node.value;
                let conf_changed = (self_node.fast_confidence - other_node.fast_confidence).abs() > 1e-6;

                if value_changed || conf_changed {
                    modified.push(BeliefModification {
                        id: self_node.id,
                        key: self_node.key.clone(),
                        old_value: other_node.value.clone(),
                        new_value: self_node.value.clone(),
                        old_confidence: other_node.fast_confidence,
                        new_confidence: self_node.fast_confidence,
                    });
                }

                if conf_changed {
                    confidence_delta.insert(
                        self_node.key.clone(),
                        (other_node.fast_confidence, self_node.fast_confidence),
                    );
                }
            }
        }

        BeliefQuadDiff {
            added,
            removed,
            modified,
            // Edge diffs: Phase 1 — empty (edge serialization deferred to Phase 5, TD-004)
            semantic_edges: SemanticEdgeDiff::default(),
            temporal_edges: TemporalEdgeDiff::default(),
            causal_edges: CausalEdgeDiff::default(),
            entity_edges: EntityEdgeDiff::default(),
            confidence_delta,
            trajectory_ece: None,
        }
    }
}
