//! Forget policy: right to erasure with causal reachability verification.
//!
//! The Mnemonic Sovereignty survey (arXiv:2604.16548) identifies verifiable deletion
//! — guaranteeing a belief is not recoverable via any path in the BeliefQuad,
//! including the causal graph — as one of the most open research problems in the
//! field. Epica implements this using a post-deletion traversal of the CausalGraph.
//!
//! **Scope of verification**: `ForgetVerificationResult::Verified` guarantees
//! in-process graph erasure only. If the runtime is backed by Redis, disk snapshots,
//! or shared cross-agent copies, those backends are not queried here. When persistence
//! backends are active, `execute_with_backends()` returns `PersistenceNotVerified`
//! to make this limitation explicit. See DEVLOG HD-S6 for the `PersistenceVerifier`
//! trait design planned for Phase 7.

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
    /// In-process graph erasure confirmed: the belief is no longer reachable via
    /// any causal path in the `BeliefQuad`. No persistence backends were active.
    Verified,

    /// In-process graph erasure confirmed, but one or more persistence backends
    /// (e.g. Redis, disk snapshots, cross-agent replicas) were not queried.
    /// The belief may persist in the listed backends and must be erased separately.
    PersistenceNotVerified { backends: Vec<String> },

    /// The belief is still reachable in the graph via the listed causal paths.
    PartialDeletion { remaining_paths: Vec<Vec<BeliefId>> },

    /// Verification itself failed (e.g. graph traversal error).
    VerificationError(String),
}

/// Policy for erasing beliefs from the quad.
pub struct ForgetPolicy {
    pub triggers: Vec<ForgetTrigger>,
    /// Post-deletion verifier: confirms no path in the BeliefQuad can still reach the belief.
    ///
    /// **Scope**: verifies in-process graph reachability only. Does not query Redis,
    /// disk snapshots, or cross-agent copies. Use `execute_with_backends()` to
    /// surface the persistence coverage gap explicitly.
    pub verify_fn: Box<dyn Fn(&BeliefId, &epica_core::BeliefQuad) -> ForgetVerificationResult + Send + Sync>,
}

impl ForgetPolicy {
    /// Verify causal safety, then remove `id` from the quad.
    ///
    /// The verification runs **before** removal so the causal graph is still
    /// intact when we traverse it. The removal is unconditional — the caller
    /// decides whether to cascade deletions based on the returned result.
    ///
    /// Returns in-process graph verification only. If persistence backends are
    /// configured, use `execute_with_backends()` instead.
    pub fn execute(
        &self,
        id: BeliefId,
        quad: &mut epica_core::BeliefQuad,
    ) -> ForgetVerificationResult {
        let result = (self.verify_fn)(&id, quad);
        quad.remove(id);
        result
    }

    /// Verify causal safety, remove `id`, and report any unchecked backends.
    ///
    /// When `active_backends` is non-empty, `Verified` is demoted to
    /// `PersistenceNotVerified` — making the coverage gap explicit to the caller
    /// rather than silently returning a guarantee that does not hold end-to-end.
    pub fn execute_with_backends(
        &self,
        id: BeliefId,
        quad: &mut epica_core::BeliefQuad,
        active_backends: Vec<String>,
    ) -> ForgetVerificationResult {
        let result = self.execute(id, quad);
        if active_backends.is_empty() {
            result
        } else {
            match result {
                ForgetVerificationResult::Verified => {
                    ForgetVerificationResult::PersistenceNotVerified {
                        backends: active_backends,
                    }
                }
                other => other,
            }
        }
    }

    /// Returns `true` if `trigger` is in this policy's trigger list.
    pub fn is_triggered_by(&self, trigger: &ForgetTrigger) -> bool {
        use std::mem::discriminant;
        self.triggers.iter().any(|t| discriminant(t) == discriminant(trigger))
    }
}

/// Default pre-deletion verifier using causal graph reachability.
///
/// **Scope**: verifies in-process graph erasure only. Does not query Redis,
/// disk snapshots, or cross-agent copies. A `Verified` result means the
/// belief is not reachable in the `BeliefQuad`; it does not mean the belief
/// has been erased from all persistence backends.
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
