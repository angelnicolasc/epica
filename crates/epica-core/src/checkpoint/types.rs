use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::quad::BeliefQuad;

/// Opaque identifier for a [`BeliefQuadCheckpoint`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CheckpointId(Uuid);

impl CheckpointId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for CheckpointId {
    fn default() -> Self {
        Self::new()
    }
}

impl CheckpointId {
    /// Parse from the wire format `"chk:{uuid}"` or bare `"{uuid}"`.
    pub fn parse(s: &str) -> Option<Self> {
        let uuid_str = s.strip_prefix("chk:").unwrap_or(s);
        Uuid::parse_str(uuid_str).ok().map(Self)
    }

    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl std::fmt::Display for CheckpointId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "chk:{}", self.0)
    }
}

/// An immutable snapshot of the [`BeliefQuad`] at a specific version.
///
/// The `quad` is boxed to break the recursive size: `BeliefQuad` contains
/// `Vec<BeliefQuadCheckpoint>` which would otherwise be infinitely sized.
#[derive(Debug, Clone)]
pub struct BeliefQuadCheckpoint {
    pub id: CheckpointId,
    pub version: u64,
    pub timestamp_ms: u64,
    /// The full quad state at checkpoint time, boxed to break the recursive type.
    pub quad: Box<BeliefQuad>,
}

impl BeliefQuadCheckpoint {
    /// Capture an immutable snapshot of `quad` at its current version.
    ///
    /// The snapshot strips its own nested checkpoint history (see the body of
    /// this method) to avoid the O(2^n) recursive blow-up that would otherwise
    /// hit after a handful of captures. Historical checkpoints stay accessible
    /// on the live quad — individual snapshots simply don't carry sub-history.
    ///
    /// # Example
    ///
    /// ```
    /// use epica_core::{BeliefNode, BeliefQuad, BeliefQuadCheckpoint, BeliefValue, Provenance};
    ///
    /// let mut quad = BeliefQuad::new();
    /// quad.insert(BeliefNode::new(
    ///     "k", BeliefValue::Asserted("v".into()),
    ///     Provenance::UserStatement { turn: 0 }, 0.9,
    /// ));
    ///
    /// let cp = BeliefQuadCheckpoint::capture(&quad);
    /// assert_eq!(cp.version, quad.version());
    /// assert_eq!(cp.quad.len(), 1);
    /// ```
    pub fn capture(quad: &BeliefQuad) -> Self {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        // Clone the quad but strip its nested checkpoints.
        //
        // Without this, every checkpoint recursively embeds all previous
        // checkpoints, producing 2^n nested clones after n update_belief()
        // calls — exponential memory and a stack-overflow on drop.
        // Historical checkpoints remain accessible on the *live* BeliefQuad;
        // individual snapshots don't need their own sub-history.
        let mut snap = quad.clone();
        snap.checkpoints.clear();

        Self {
            id: CheckpointId::new(),
            version: quad.version(),
            timestamp_ms: now_ms,
            quad: Box::new(snap),
        }
    }
}
