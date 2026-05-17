//! Belief predicate trait for contract pre/postconditions.

use epica_core::BeliefQuad;

/// A predicate that can be evaluated against a `BeliefQuad`.
///
/// Used for contract preconditions (P) and session invariants (I).
pub trait BeliefPredicate: Send + Sync {
    fn evaluate(&self, quad: &BeliefQuad) -> bool;
    fn description(&self) -> &str;
}

/// A predicate that checks whether a specific belief key exists.
pub struct BeliefExists {
    pub key: String,
}

impl BeliefPredicate for BeliefExists {
    fn evaluate(&self, quad: &BeliefQuad) -> bool {
        quad.iter().any(|(_, n)| n.key == self.key)
    }

    fn description(&self) -> &str {
        &self.key
    }
}

/// A predicate that checks minimum confidence for a belief key.
pub struct MinConfidence {
    pub key: String,
    pub threshold: f32,
}

impl BeliefPredicate for MinConfidence {
    fn evaluate(&self, quad: &BeliefQuad) -> bool {
        quad.iter()
            .find(|(_, n)| n.key == self.key)
            .map(|(_, n)| n.fast_confidence >= self.threshold)
            .unwrap_or(false)
    }

    fn description(&self) -> &str {
        &self.key
    }
}
