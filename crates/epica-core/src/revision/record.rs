use std::collections::HashSet;

use uuid::Uuid;

use crate::belief::{BeliefId, BeliefValue};
use super::postulates::PostulateAudit;

/// Immutable record of a single AGM belief revision.
///
/// Stored as part of the `BeliefRuntime`'s audit trail (Phase 3 enforcement).
/// The diff between successive records can be used for root-cause analysis
/// without LLM-as-judge.
#[derive(Debug, Clone)]
pub struct RevisionRecord {
    pub revision_id: Uuid,
    pub belief_id: BeliefId,
    pub old_value: BeliefValue,
    pub new_value: BeliefValue,
    pub old_confidence: f32,
    pub new_confidence: f32,
    /// Set of beliefs that were contracted (minimally removed) before expansion.
    /// Empty for expand-only revisions (K*4 vacuity).
    pub contracted: HashSet<BeliefId>,
    /// Postulate audit captured against the pre-revision quad state.
    pub postulate_audit: PostulateAudit,
    pub timestamp_ms: u64,
    pub from_version: u64,
    pub to_version: u64,
}

impl RevisionRecord {
    /// Returns `true` if this revision was expand-only (no contraction performed).
    pub fn is_expansion(&self) -> bool {
        self.contracted.is_empty()
    }
}
