//! Confidence history for Trajectory-ECE (T-ECE) computation.
//!
//! `BeliefRuntime::update_belief()` appends a record for every revision.
//! When a belief is subsequently contradicted, `mark_correct(id, false)` is
//! called for the most recent record of that belief, providing ground-truth
//! for the ECE computation.  At session end, call `finalize_session()` to
//! mark surviving beliefs (never contradicted) as correct.

use std::time::{SystemTime, UNIX_EPOCH};

use epica_core::BeliefId;

/// One data point in the per-session confidence trace.
#[derive(Debug, Clone)]
pub struct ConfidenceRecord {
    pub belief_id: BeliefId,
    pub confidence: f32,
    /// Outcome known retroactively:
    /// - `None`  — outcome not yet determined
    /// - `Some(true)`  — belief was not subsequently contradicted (correct)
    /// - `Some(false)` — belief was revised away via a contradiction (incorrect)
    pub was_correct: Option<bool>,
    pub timestamp_ms: u64,
}

/// Per-session confidence trace used to compute Trajectory-ECE.
///
/// T-ECE = Σ_t |confidence_t − accuracy_t| / T  (Agentic UQ, arXiv:2601.15703)
///
/// Target for well-calibrated sessions: T-ECE < 0.08 on > 20 steps
/// (BeliefShift benchmark, March 2026).
#[derive(Debug, Default)]
pub struct ConfidenceHistory {
    records: Vec<ConfidenceRecord>,
}

impl ConfidenceHistory {
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a confidence data point for `belief_id`.
    pub fn push(&mut self, belief_id: BeliefId, confidence: f32) {
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        self.records.push(ConfidenceRecord {
            belief_id,
            confidence,
            was_correct: None,
            timestamp_ms,
        });
    }

    /// Retroactively set the outcome for the most recent record of `id`.
    ///
    /// Called automatically by `update_belief()` when a contradiction revision
    /// occurs (sets `correct = false`).  Can also be called by external
    /// oracle code to supply ground-truth (e.g. in tests or eval harnesses).
    pub fn mark_correct(&mut self, id: BeliefId, correct: bool) {
        if let Some(r) = self.records.iter_mut().rev().find(|r| r.belief_id == id) {
            r.was_correct = Some(correct);
        }
    }

    /// Replace the confidence of the most recent record for `id`.
    ///
    /// Called by `apply_system2_result()` to supersede the fast confidence
    /// that was pushed by `update_belief()` when System 2 was triggered.
    /// The revised System 2 confidence replaces — rather than appends to —
    /// the trajectory, so T-ECE reflects the recalibrated estimate.
    pub fn replace_last_confidence(&mut self, id: BeliefId, confidence: f32) {
        if let Some(r) = self.records.iter_mut().rev().find(|r| r.belief_id == id) {
            r.confidence = confidence;
        }
    }

    /// Mark all records with an unknown outcome as correct.
    ///
    /// Call at session end: beliefs that survived the full session without
    /// ever being contradicted are assumed to have been correct.  This
    /// converts `was_correct: None` entries into ground-truth for T-ECE.
    pub fn finalize_session(&mut self) {
        for r in &mut self.records {
            if r.was_correct.is_none() {
                r.was_correct = Some(true);
            }
        }
    }

    /// Extract `(confidence, was_correct)` pairs for `tece::compute()`.
    /// Only records with a known outcome are included.
    pub fn as_tece_input(&self) -> Vec<(f32, bool)> {
        self.records
            .iter()
            .filter_map(|r| r.was_correct.map(|c| (r.confidence, c)))
            .collect()
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}
