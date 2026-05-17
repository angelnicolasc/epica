//! Multicriteria belief retrieval.

/// Scored belief for retrieval ranking.
#[derive(Debug, Clone)]
pub struct ScoredBelief {
    pub id: epica_core::BeliefId,
    pub score: f32,
    pub prospective_sim: f32,
    pub uncertainty_bonus: f32,
    pub causal_centrality: f32,
    pub decay_penalty: f32,
}

/// Compute the retrieval score for a belief.
///
/// Score = prospective_sim   × 0.45
///       + uncertainty_bonus × 0.25   (high uncertainty → prioritise reflection)
///       + causal_centrality × 0.20   (structurally important beliefs surface first)
///       - decay_penalty     × 0.10   (stale beliefs are penalised)
///
/// Weights follow the Kumiho playbook (arXiv:2603.17244).
/// `prospective_sim` will be 0.0 until Phase 4 embeddings are wired in (TD-001).
pub fn compute_score(
    prospective_sim: f32,
    fast_confidence: f32,
    causal_centrality: f32,
    decay_factor: f32,
) -> f32 {
    let uncertainty_bonus = 1.0 - fast_confidence;
    let decay_penalty = 1.0 - decay_factor;
    prospective_sim * 0.45
        + uncertainty_bonus * 0.25
        + causal_centrality * 0.20
        - decay_penalty * 0.10
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn score_fully_confident_fresh_belief() {
        // confidence=1.0, decay=1.0 (fresh), centrality=0.0, no prospective
        let s = compute_score(0.0, 1.0, 0.0, 1.0);
        // uncertainty_bonus = 0, decay_penalty = 0  → score = 0
        assert!((s - 0.0).abs() < 1e-6, "score={s}");
    }

    #[test]
    fn score_uncertain_central_fresh_belief() {
        let s = compute_score(0.0, 0.5, 1.0, 1.0);
        // 0 + 0.5*0.25 + 1.0*0.20 - 0 = 0.325
        assert!((s - 0.325).abs() < 1e-5, "score={s}");
    }
}
