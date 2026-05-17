use crate::{diff::BeliefQuadDiff, quad::BeliefQuad, revision::error::RollbackError};

use super::types::{BeliefQuadCheckpoint, CheckpointId};

/// Checkpoint and rollback operations on `BeliefQuad`.
///
/// Rollback is **formal AGM-compliant belief contraction**, not a memcpy.
/// The K*4 vacuity guard ensures we never contract beliefs unnecessarily.
impl BeliefQuad {
    /// Save the current state as an immutable checkpoint.
    ///
    /// The checkpoint is appended to `self.checkpoints` and can be retrieved
    /// via `rollback_to(id)`. Checkpoints accumulate until explicitly cleared.
    pub fn checkpoint(&mut self) -> CheckpointId {
        let cp = BeliefQuadCheckpoint::capture(self);
        let id = cp.id;
        self.checkpoints.push(cp);
        id
    }

    /// Rollback to a previously saved checkpoint.
    ///
    /// ## AGM compliance
    ///
    /// - Computes the diff between current state and checkpoint state.
    /// - If there are no contradictions (`!diff.has_contradictions()`), the rollback
    ///   would unnecessarily contract beliefs — K*4 violation, returns `Err`.
    /// - Otherwise, replaces `*self` with the checkpoint state and returns the diff
    ///   as a root-cause report.
    ///
    /// The diff IS the root-cause analysis — no LLM-as-judge needed.
    pub fn rollback_to(&mut self, checkpoint_id: CheckpointId) -> Result<BeliefQuadDiff, RollbackError> {
        // Clone the checkpoint's quad first to release the borrow on self
        let snap_quad = self
            .checkpoints
            .iter()
            .find(|c| c.id == checkpoint_id)
            .map(|c| c.quad.clone())
            .ok_or(RollbackError::CheckpointNotFound(checkpoint_id))?;

        let diff = self.diff(&snap_quad);

        if !diff.has_contradictions() {
            return Err(RollbackError::UnnecessaryContraction(diff));
        }

        *self = *snap_quad;
        Ok(diff)
    }

    /// Remove all checkpoints. Call after a successful run to reclaim memory.
    pub fn clear_checkpoints(&mut self) {
        self.checkpoints.clear();
    }

    /// Number of stored checkpoints.
    pub fn checkpoint_count(&self) -> usize {
        self.checkpoints.len()
    }

    /// Find a stored checkpoint by ID.
    pub fn checkpoint_by_id(&self, id: CheckpointId) -> Option<&BeliefQuadCheckpoint> {
        self.checkpoints.iter().find(|c| c.id == id)
    }

    /// Iterate over all stored checkpoints in insertion order.
    pub fn checkpoints(&self) -> &[BeliefQuadCheckpoint] {
        &self.checkpoints
    }
}
