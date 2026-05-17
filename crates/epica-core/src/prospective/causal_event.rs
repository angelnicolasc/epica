use serde::{Deserialize, Serialize};

use crate::belief::BeliefId;

/// A structured causal event within a prospective scenario.
///
/// Kumiho (arXiv:2603.17244) insight: simple narrative compression loses causal detail.
/// Storing events as `(trigger, consequence, confidence)` triples preserves the
/// causal structure needed for accurate retrieval.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CausalEvent {
    /// Natural-language description of what triggers this event.
    pub trigger: String,

    /// Natural-language description of the consequence.
    pub consequence: String,

    /// Estimated probability of this event given the parent belief.
    pub probability: f32,

    /// Beliefs that are preconditions for this event (if known at index time).
    pub precondition_beliefs: Vec<BeliefId>,
}
