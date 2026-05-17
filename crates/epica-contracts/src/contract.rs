//! BehavioralContract: the C=(P,I,G,R) formalism.
//!
//! From Agent Behavioral Contracts (arXiv:2602.22302).

use crate::{
    governance::GovernancePolicies,
    invariant::{ContractViolation, SessionInvariant, ViolationClass},
    predicate::BeliefPredicate,
    recovery::RecoveryPolicy,
};

/// A typed behavioral contract for a `BeliefRuntime`.
///
/// Satisfies (p, δ, k)-satisfaction from ABC paper:
/// probability `p` that no more than `δ` violations occur in `k` steps.
///
/// Unlike AgentAssert (Python, output-level wrapper), this contract operates on
/// *belief mutations* — before the agent acts, not after.
pub struct BehavioralContract {
    pub domain: String,

    /// P — Preconditions: must hold in the BeliefQuad before any action.
    pub preconditions: Vec<Box<dyn BeliefPredicate>>,

    /// I — Invariants: must hold at every `update_belief()` call.
    pub invariants: Vec<SessionInvariant>,

    /// G — Governance: resource limits and authorization.
    pub governance: GovernancePolicies,

    /// R — Recovery: action taken when an invariant is violated.
    pub recovery: RecoveryPolicy,

    /// Probabilistic satisfaction parameters.
    pub p: f64,     // minimum probability of compliance
    pub delta: u32, // max violations allowed in window
    pub k: u32,     // window size in steps

    /// α: natural drift rate of the agent.
    pub alpha: f64,
    /// γ: enforcement rate of this contract.
    pub gamma: f64,
}

impl BehavioralContract {
    /// Drift bound D* = α/γ with Gaussian concentration estimate.
    pub fn drift_bound(&self) -> crate::drift::DriftBound {
        crate::drift::DriftBound::new(self.alpha, self.gamma)
    }

    /// Returns `true` if all preconditions hold against the current quad state.
    pub fn check_preconditions(&self, quad: &epica_core::BeliefQuad) -> bool {
        self.preconditions.iter().all(|p| p.evaluate(quad))
    }

    /// Evaluate all invariants and return the first Hard or Critical violation.
    ///
    /// Soft violations are skipped — they are logged by the caller, not returned.
    /// Pass `last_checkpoint` so violations can reference the rollback target.
    pub fn check_invariants(
        &self,
        quad: &epica_core::BeliefQuad,
        last_checkpoint: Option<epica_core::CheckpointId>,
    ) -> Option<ContractViolation> {
        for inv in &self.invariants {
            if !inv.predicate.evaluate(quad)
                && matches!(inv.class, ViolationClass::Hard | ViolationClass::Critical)
            {
                return Some(ContractViolation {
                    violation_id: uuid::Uuid::new_v4(),
                    invariant_description: inv.predicate.description().to_string(),
                    class: inv.class,
                    at_version: quad.version(),
                    last_valid_checkpoint: last_checkpoint,
                });
            }
        }
        None
    }

    /// Collect **all** invariant violations across every class (Soft, Hard, Critical).
    ///
    /// Unlike `check_invariants()`, this does not stop at the first Hard/Critical.
    /// The `ContractEngine` uses this to:
    /// - Log all Soft violations (ABC paper baseline: 5.2–6.8 per session).
    /// - Trigger recovery on the first Hard violation.
    /// - Halt on the first Critical violation.
    pub fn collect_all_violations(
        &self,
        quad: &epica_core::BeliefQuad,
        last_checkpoint: Option<epica_core::CheckpointId>,
    ) -> Vec<ContractViolation> {
        self.invariants
            .iter()
            .filter_map(|inv| {
                if !inv.predicate.evaluate(quad) {
                    Some(ContractViolation {
                        violation_id: uuid::Uuid::new_v4(),
                        invariant_description: inv.predicate.description().to_string(),
                        class: inv.class,
                        at_version: quad.version(),
                        last_valid_checkpoint: last_checkpoint,
                    })
                } else {
                    None
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{governance::GovernancePolicies, predicate::MinConfidence, recovery::RecoveryPolicy};
    use epica_core::{BeliefNode, BeliefQuad, BeliefValue, Provenance};

    fn minimal_contract() -> BehavioralContract {
        BehavioralContract {
            domain: "test".into(),
            preconditions: vec![],
            invariants: vec![],
            governance: GovernancePolicies::default(),
            recovery: RecoveryPolicy::RollbackToLastCheckpoint,
            p: 0.99,
            delta: 1,
            k: 100,
            alpha: 0.05,
            gamma: 0.5,
        }
    }

    #[test]
    fn drift_bound_does_not_panic() {
        let c = minimal_contract();
        let b = c.drift_bound();
        assert!((b.expected_drift - 0.1).abs() < 1e-10);
    }

    #[test]
    fn empty_preconditions_always_pass() {
        let c = minimal_contract();
        assert!(c.check_preconditions(&BeliefQuad::new()));
    }

    #[test]
    fn empty_invariants_no_violation() {
        let c = minimal_contract();
        assert!(c.check_invariants(&BeliefQuad::new(), None).is_none());
    }

    #[test]
    fn precondition_fails_when_key_missing() {
        let mut c = minimal_contract();
        c.preconditions.push(Box::new(MinConfidence {
            key: "required_key".into(),
            threshold: 0.5,
        }));
        assert!(!c.check_preconditions(&BeliefQuad::new()));
    }

    #[test]
    fn precondition_passes_when_key_present() {
        let mut c = minimal_contract();
        c.preconditions.push(Box::new(MinConfidence {
            key: "my_key".into(),
            threshold: 0.5,
        }));
        let mut quad = BeliefQuad::new();
        quad.insert(BeliefNode::new(
            "my_key",
            BeliefValue::Asserted("x".into()),
            Provenance::UserStatement { turn: 0 },
            0.8,
        ));
        assert!(c.check_preconditions(&quad));
    }
}
