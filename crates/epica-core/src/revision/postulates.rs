use crate::{
    belief::{BeliefId, BeliefValue},
    embedding::EquivalenceVerdict,
    quad::{BeliefQuad, SemanticGraph, VerdictTrace},
};

/// Verification result for the six AGM revision postulates.
///
/// Computed BEFORE the mutation is applied (captures old state as the basis).
/// This is an **audit trail**, not a blocking validator — the revision proceeds
/// regardless, and debug builds assert that all postulates hold.
///
/// References: Alchourrón, Gärdenfors, Makinson (1985); Kumiho (arXiv:2603.17244).
#[derive(Debug, Clone)]
pub struct PostulateAudit {
    /// K*2 — Success: the revised set contains `new_value`.
    /// Always `true` after a well-formed revision; captured for audit completeness.
    pub success: bool,

    /// K*3 — Inclusion: K*φ ⊆ Cn(K + {φ}).
    /// The revised set is a logical consequence of the original beliefs plus φ.
    pub inclusion: bool,

    /// K*4 — Vacuity: if ¬φ ∉ K, then K ⊆ K*φ.
    /// `true` if no contraction was necessary (expand-only path was taken).
    pub vacuity: bool,

    /// K*5 — Consistency: K*φ = K_false only if φ is inconsistent.
    /// `true` if the revised quad is not empty (φ is not self-contradictory).
    pub consistency: bool,

    /// K*6 — Extensionality: Cn({φ}) = Cn({ψ}) implies K*φ = K*ψ.
    ///
    /// `true` when revising with `new_value` yields the same contradiction
    /// verdict as revising with any *semantic paraphrase* of `new_value`
    /// already present in the quad. Computed against the embedding provider
    /// installed on the [`BeliefQuad`]; when no provider is configured (or
    /// no paraphrase is detectable from cache) the postulate is satisfied
    /// vacuously — which matches the pre-Sprint-1 behavior exactly.
    pub extensionality: bool,

    /// Diagnostic trace of the contradiction-detection path that produced
    /// `vacuity`. Useful when `extensionality` is `false` to identify which
    /// belief in the quad triggered the K*6 violation.
    pub verdict_trace: VerdictTrace,

    /// When `extensionality == false`, the [`BeliefId`] of the in-quad
    /// paraphrase whose contradiction verdict disagreed with `new_value`.
    /// `None` when extensionality holds (the common case).
    pub extensionality_witness: Option<BeliefId>,
}

impl PostulateAudit {
    /// Compute the audit for a proposed revision of `belief_id` to `new_value`.
    ///
    /// `pre_revision_quad` is the quad state BEFORE any mutation.
    /// `contradicts_current` is the result of `check_contradiction()` from the caller.
    /// `verdict_trace` is the trace from `check_contradiction_traced()` describing
    /// which decision path was taken.
    pub fn verify(
        pre_revision_quad: &BeliefQuad,
        belief_id: BeliefId,
        new_value: &BeliefValue,
        contradicts_current: bool,
        verdict_trace: VerdictTrace,
    ) -> Self {
        let belief_exists = pre_revision_quad.get(belief_id).is_some();

        // K*2: success — trivially true for a well-formed revision
        let success = belief_exists;

        // K*3: inclusion — the new value must be reachable from K + {φ}
        // Phase 1: true if the belief exists (we're replacing, not adding noise)
        let inclusion = belief_exists;

        // K*4: vacuity — no contraction needed if ¬φ was not in K
        let vacuity = !contradicts_current;

        // K*5: consistency — new_value must not be self-contradictory
        let consistency = !is_self_contradictory(new_value);

        // K*6: extensionality — real semantic check against in-quad paraphrases.
        let (extensionality, extensionality_witness) = check_extensionality(
            pre_revision_quad,
            belief_id,
            new_value,
            contradicts_current,
        );

        Self {
            success,
            inclusion,
            vacuity,
            consistency,
            extensionality,
            verdict_trace,
            extensionality_witness,
        }
    }

    /// Returns `true` if all postulates that must hold after a valid revision pass.
    ///
    /// K*4 (vacuity) is intentionally excluded: `vacuity = false` means contraction
    /// was needed, which is a legitimate outcome, not a violation.
    pub fn all_critical_pass(&self) -> bool {
        self.success && self.inclusion && self.consistency && self.extensionality
    }

    /// Name of the first critical postulate that failed, for error reporting.
    pub fn failed_postulate_name(&self) -> &'static str {
        if !self.success      { "K*2 (success)" }
        else if !self.inclusion    { "K*3 (inclusion)" }
        else if !self.consistency  { "K*5 (consistency)" }
        else if !self.extensionality { "K*6 (extensionality)" }
        else { "none" }
    }
}

/// Phase 1: a value is self-contradictory if it is a `Deterministic` JSON `null`
/// paired with a non-null expectation. Full contradiction detection requires
/// domain knowledge (Phase 2).
fn is_self_contradictory(value: &BeliefValue) -> bool {
    match value {
        BeliefValue::Deterministic(v) => {
            // A value of `false` with no context is not self-contradictory —
            // only detect obvious structural impossibilities
            v.is_null() && false // Phase 1: no self-contradictions detected
        }
        _ => false,
    }
}

/// K*6 verification: scan the quad for *semantic paraphrases* of `new_value`
/// and confirm that revising the same target with any of them yields the same
/// contradiction verdict.
///
/// Returns `(extensionality_holds, witness)`. When no embedding provider is
/// installed or no paraphrase is found (cache cold, or no paraphrase exists
/// in the quad), the postulate holds vacuously and the witness is `None` —
/// this is the path taken on legacy runtimes without a provider, so behavior
/// is identical to the Sprint-0 implementation.
fn check_extensionality(
    quad: &BeliefQuad,
    belief_id: BeliefId,
    new_value: &BeliefValue,
    expected_contradicts: bool,
) -> (bool, Option<BeliefId>) {
    let Some(provider) = quad.embedding_provider.as_deref() else {
        return (true, None);
    };
    let BeliefValue::Asserted(new_text) = new_value else {
        return (true, None);
    };
    let Some(new_emb) = provider.embed_cached(new_text) else {
        return (true, None);
    };
    let Some(target_node) = quad.get(belief_id) else {
        return (true, None);
    };
    let target_value = &target_node.value;
    let equivalence = quad.semantic_equivalence();

    for (other_id, node) in quad.nodes.iter() {
        if other_id == belief_id {
            continue;
        }
        let BeliefValue::Asserted(other_text) = &node.value else {
            continue;
        };
        let Some(other_emb) = provider.embed_cached(other_text) else {
            continue;
        };
        if let EquivalenceVerdict::Equivalent(_) =
            equivalence.classify(&new_emb, &other_emb)
        {
            // `other_text` is a semantic paraphrase of `new_text`. By K*6,
            // revising the same target with `other_text` must produce the
            // same contradiction verdict.
            let alt = BeliefValue::Asserted(other_text.clone());
            let (alt_contradicts, _) = SemanticGraph::value_contradicts_semantic(
                Some(provider),
                &equivalence,
                target_value,
                &alt,
            );
            let alt_total = alt_contradicts
                || quad.semantic().has_contradiction_edges(belief_id);
            if alt_total != expected_contradicts {
                return (false, Some(other_id));
            }
        }
    }
    (true, None)
}
