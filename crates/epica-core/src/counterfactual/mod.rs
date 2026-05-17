use std::collections::HashSet;

use crate::{belief::BeliefId, quad::BeliefQuad};

/// A counterfactual world view from which a specific belief and all its causal
/// descendants have been excluded.
///
/// Created by [`BeliefQuad::counterfactual_query`]. Represents the answer to:
/// "what would the agent believe if `antecedent` had never occurred?"
#[derive(Debug)]
pub struct CounterfactualWorld<'a> {
    /// The belief that was counterfactually removed.
    pub antecedent: BeliefId,

    /// All beliefs excluded from this world view (antecedent + its causal descendants).
    pub excluded: HashSet<BeliefId>,

    /// Reference to the original quad (for reading surviving beliefs).
    quad: &'a BeliefQuad,
}

impl<'a> CounterfactualWorld<'a> {
    /// Iterate over beliefs that survive in this counterfactual world.
    pub fn surviving_beliefs(&'a self) -> impl Iterator<Item = (BeliefId, &'a crate::belief::BeliefNode)> + 'a {
        let excluded = &self.excluded;
        self.quad
            .nodes
            .iter()
            .filter(move |(id, _)| !excluded.contains(id))
    }

    /// Number of beliefs surviving in this world.
    pub fn surviving_count(&self) -> usize {
        self.quad.len() - self.excluded.len()
    }
}

/// Counterfactual query implementation on `BeliefQuad`.
impl BeliefQuad {
    /// Answer: "what would the agent believe if `antecedent` had never occurred?"
    ///
    /// Computes the set of causal descendants of `antecedent` and returns a view
    /// that excludes them. No LLM calls — pure traversal of the CausalGraph.
    pub fn counterfactual_query(&self, antecedent: BeliefId) -> CounterfactualWorld<'_> {
        let mut excluded = self.causal.descendants_of(antecedent);
        excluded.insert(antecedent);
        CounterfactualWorld {
            antecedent,
            excluded,
            quad: self,
        }
    }
}
