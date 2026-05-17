use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

use ordered_float::NotNan;

use uuid::Uuid;

use crate::{
    belief::{BeliefId, BeliefValue, Provenance},
    quad::BeliefQuad,
};

use super::{
    error::BeliefRevisionError,
    postulates::PostulateAudit,
    record::RevisionRecord,
};

/// AGM belief revision implemented on `BeliefQuad`.
///
/// Satisfies postulates K*2–K*6 and Hansson's Relevance/Core-Retainment.
/// Revision follows the Levi identity: contract-then-expand when the new information
/// contradicts the current belief state; expand-only otherwise.
impl BeliefQuad {
    /// Revise the belief at `id` to hold `new_value` with the given `provenance`
    /// and `new_confidence`.
    ///
    /// ## Algorithm (Levi identity)
    ///
    /// 1. **K*4 check**: does `new_value` contradict the current belief state?
    ///    - No → `expand_only()` (current beliefs are preserved as a superset)
    ///    - Yes → compute `minimal_contraction_set()` and contract, then expand
    ///
    /// 2. **Expansion**: update `id`'s value, provenance, and confidence; bump version.
    ///
    /// 3. **Invalidation**: clear the prospective index entry for `id` (regenerated
    ///    at write-time in Phase 4).
    pub fn revise(
        &mut self,
        belief_id: BeliefId,
        new_value: BeliefValue,
        new_provenance: Provenance,
        new_confidence: f32,
    ) -> Result<RevisionRecord, BeliefRevisionError> {
        if self.nodes.get(belief_id).is_none() {
            return Err(BeliefRevisionError::BeliefNotFound);
        }

        let contradicts = self.check_contradiction(belief_id, &new_value);

        // Run postulate audit against the pre-revision state.
        // K*4 (vacuity) is informational — false means contraction is needed, which is legitimate.
        // K*2, K*3, K*5, K*6 must hold in every build; violations are a runtime logic error.
        let audit = PostulateAudit::verify(self, belief_id, &new_value, contradicts);
        if !audit.all_critical_pass() {
            return Err(BeliefRevisionError::PostulateViolation {
                postulate: audit.failed_postulate_name(),
            });
        }

        if !contradicts {
            return self.expand_only(belief_id, new_value, new_provenance, new_confidence, audit);
        }

        // Contract: remove the minimal set of beliefs that entail ¬new_value
        let contraction_set = self.minimal_contraction_set(belief_id);
        self.apply_contraction(&contraction_set);

        // Expand: update the belief with the new information
        let old_version = self.version;
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let node = self.nodes.get_mut(belief_id).ok_or(BeliefRevisionError::BeliefNotFound)?;
        let old_value = node.value.clone();
        let old_confidence = node.fast_confidence;

        node.value = new_value.clone();
        node.provenance = new_provenance;
        node.fast_confidence = new_confidence.clamp(0.0, 1.0);
        node.slow_confidence = None; // reset System 2; it must re-reflect on the new value
        self.version += 1;

        self.prospective_index.invalidate(belief_id);

        Ok(RevisionRecord {
            revision_id: Uuid::new_v4(),
            belief_id,
            old_value,
            new_value,
            old_confidence,
            new_confidence,
            contracted: contraction_set,
            postulate_audit: audit,
            timestamp_ms: now_ms,
            from_version: old_version,
            to_version: self.version,
        })
    }

    /// Expand-only revision: add `new_value` without contracting any beliefs.
    ///
    /// Called when K*4 vacuity holds (the current belief state does not contain ¬new_value).
    fn expand_only(
        &mut self,
        belief_id: BeliefId,
        new_value: BeliefValue,
        new_provenance: Provenance,
        new_confidence: f32,
        audit: PostulateAudit,
    ) -> Result<RevisionRecord, BeliefRevisionError> {
        let old_version = self.version;
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let node = self.nodes.get_mut(belief_id).ok_or(BeliefRevisionError::BeliefNotFound)?;
        let old_value = node.value.clone();
        let old_confidence = node.fast_confidence;

        node.value = new_value.clone();
        node.provenance = new_provenance;
        node.fast_confidence = new_confidence.clamp(0.0, 1.0);
        self.version += 1;

        self.prospective_index.invalidate(belief_id);

        Ok(RevisionRecord {
            revision_id: Uuid::new_v4(),
            belief_id,
            old_value,
            new_value,
            old_confidence,
            new_confidence,
            contracted: HashSet::new(), // K*4: no contraction
            postulate_audit: audit,
            timestamp_ms: now_ms,
            from_version: old_version,
            to_version: self.version,
        })
    }

    /// Compute the minimal set of beliefs to contract before accepting `new_value` for `belief_id`.
    ///
    /// `inferred_from_premises(belief_id)` returns `Vec<Vec<BeliefId>>` — each inner `Vec` is
    /// the premise list of one independent `InferredFrom` causal edge (one support path). To
    /// break all support for `belief_id`, we must remove at least one premise from each path
    /// (a minimal hitting set). For each path we remove the weakest premise (lowest
    /// `fast_confidence`) — the belief we least trust, and therefore lose the least by contracting.
    ///
    /// Previous implementation: `.flatten().collect()` — collected ALL premises from ALL paths,
    /// violating Hansson's Core-Retainment for beliefs with multiple independent support paths
    /// (removed more than necessary). This implementation removes exactly one premise per path.
    ///
    /// `NotNan` is used for NaN-safe `f32` comparison.
    fn minimal_contraction_set(&self, belief_id: BeliefId) -> HashSet<BeliefId> {
        self.causal
            .inferred_from_premises(belief_id)
            .into_iter()
            .filter_map(|premises| {
                premises.into_iter().min_by_key(|&id| {
                    let conf = self.nodes.get(id).map(|n| n.fast_confidence).unwrap_or(0.0);
                    NotNan::new(conf).unwrap_or(NotNan::new(0.0).unwrap())
                })
            })
            .collect()
    }

    /// Remove all beliefs in `contraction_set` from the quad.
    fn apply_contraction(&mut self, contraction_set: &HashSet<BeliefId>) {
        for &id in contraction_set {
            self.remove(id);
        }
    }
}
