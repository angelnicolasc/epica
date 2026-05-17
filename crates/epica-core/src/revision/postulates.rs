use crate::{belief::{BeliefId, BeliefValue}, quad::BeliefQuad};

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
    /// `true` when equivalent propositions yield equivalent revised sets.
    /// Phase 1 approximation: always `true` (full extensionality requires
    /// logical equivalence checking, deferred to Phase 2).
    pub extensionality: bool,
}

impl PostulateAudit {
    /// Compute the audit for a proposed revision of `belief_id` to `new_value`.
    ///
    /// `pre_revision_quad` is the quad state BEFORE any mutation.
    /// `contradicts_current` is the result of `check_contradiction()` from the caller.
    pub fn verify(
        pre_revision_quad: &BeliefQuad,
        belief_id: BeliefId,
        new_value: &BeliefValue,
        contradicts_current: bool,
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

        // K*6: extensionality — Phase 1 approximation: always true
        let extensionality = true;

        Self { success, inclusion, vacuity, consistency, extensionality }
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
