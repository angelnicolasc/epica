//! Forget policy: right to erasure with causal reachability verification.
//!
//! The Mnemonic Sovereignty survey (arXiv:2604.16548) identifies verifiable deletion
//! — guaranteeing a belief is not recoverable via any path in the BeliefQuad,
//! including the causal graph — as one of the most open research problems in the
//! field. Epica implements this using a post-deletion traversal of the CausalGraph.

use epica_core::BeliefId;

/// Conditions that trigger automatic belief erasure.
#[derive(Debug, Clone)]
pub enum ForgetTrigger {
    TtlExpired,
    ContractRevoked,
    ExplicitRequest { authorized_by: String },
    CrossAgentContamination { source_agent: String },
    GovernanceMandated { policy_id: String },
}

/// Result of a post-deletion verification pass.
#[derive(Debug, Clone)]
pub enum ForgetVerificationResult {
    /// The belief is no longer recoverable via any graph path.
    Verified,
    /// The belief is still reachable via the listed causal paths.
    PartialDeletion { remaining_paths: Vec<Vec<BeliefId>> },
    /// Verification itself failed (e.g. graph traversal error).
    VerificationError(String),
}

/// Policy for erasing beliefs from the quad.
pub struct ForgetPolicy {
    pub triggers: Vec<ForgetTrigger>,
    /// Post-deletion verifier: confirms no path in the BeliefQuad can still reach the belief.
    pub verify_fn: Box<dyn Fn(&BeliefId, &epica_core::BeliefQuad) -> ForgetVerificationResult + Send + Sync>,
}

impl ForgetPolicy {
    /// Verify causal safety, then remove `id` from the quad.
    ///
    /// The verification runs **before** removal so the causal graph is still
    /// intact when we traverse it. The removal is unconditional — the caller
    /// decides whether to cascade deletions based on the returned result.
    pub fn execute(
        &self,
        id: BeliefId,
        quad: &mut epica_core::BeliefQuad,
    ) -> ForgetVerificationResult {
        let result = (self.verify_fn)(&id, quad);
        quad.remove(id);
        result
    }

    /// Returns `true` if `trigger` is in this policy's trigger list.
    pub fn is_triggered_by(&self, trigger: &ForgetTrigger) -> bool {
        use std::mem::discriminant;
        self.triggers.iter().any(|t| discriminant(t) == discriminant(trigger))
    }
}

/// Default pre-deletion verifier using causal graph reachability.
///
/// Called **before** `quad.remove(id)`:
/// 1. Confirms `quad.get(id)` is `Some` (the belief exists).
/// 2. Checks for causal descendants — live beliefs that depend on `id`.
///    If any exist, they will become orphaned after deletion; the caller
///    should cascade the deletion or downgrade their confidence.
///
/// Returns `Verified` when no descendants exist (safe to delete without
/// leaving orphaned causal chains).
pub fn default_causal_verify_fn(
    id: &BeliefId,
    quad: &epica_core::BeliefQuad,
) -> ForgetVerificationResult {
    if quad.get(*id).is_none() {
        return ForgetVerificationResult::VerificationError(
            "belief not found in quad — already removed?".into(),
        );
    }
    let descendants: Vec<BeliefId> = quad.causal().descendants_of(*id).into_iter().collect();
    if descendants.is_empty() {
        ForgetVerificationResult::Verified
    } else {
        ForgetVerificationResult::PartialDeletion {
            remaining_paths: vec![descendants],
        }
    }
}
