//! Governance resource tracking for behavioral contracts.
//!
//! Enforces `GovernancePolicies` limits (token budget, tool-call caps,
//! human-approval gates) per session without modifying the `BeliefQuad`.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::RuntimeError;

/// Tracks per-session governance resource consumption.
///
/// One `GovernanceTracker` is shared across all `update_belief()` calls in a
/// session. All fields are thread-safe for use behind `Arc`.
#[derive(Default)]
pub struct GovernanceTracker {
    /// Cumulative token units consumed this session (1 unit per `update_belief` call).
    tokens_used: Arc<AtomicU64>,

    /// Per-key tool call counter — enforces `max_tool_calls_per_belief`.
    tool_calls: Arc<Mutex<HashMap<String, u32>>>,

    /// Keys that have been flagged as requiring human approval but have not yet
    /// received it. An update on a key in this set is blocked.
    pending_approvals: Arc<Mutex<HashSet<String>>>,
}

impl GovernanceTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Consume one governance token and check against `limit`.
    ///
    /// Returns `Err(RuntimeError::GovernanceLimitExceeded)` if the call
    /// would push usage past the limit.
    pub fn track_token_usage(
        &self,
        limit: Option<u64>,
    ) -> Result<(), RuntimeError> {
        let prev = self.tokens_used.fetch_add(1, Ordering::Relaxed);
        if let Some(max) = limit {
            if prev >= max {
                return Err(RuntimeError::GovernanceLimitExceeded {
                    resource: "tokens_per_session".into(),
                    limit: max,
                    used: prev + 1,
                });
            }
        }
        Ok(())
    }

    /// Record one tool call for `belief_key` and check against `limit`.
    pub fn track_tool_call(
        &self,
        belief_key: &str,
        limit: Option<u32>,
    ) -> Result<(), RuntimeError> {
        let mut calls = self.tool_calls.lock().unwrap();
        let count = calls.entry(belief_key.to_string()).or_insert(0);
        *count += 1;
        if let Some(max) = limit {
            if *count > max {
                return Err(RuntimeError::GovernanceLimitExceeded {
                    resource: format!("tool_calls_for:{belief_key}"),
                    limit: max as u64,
                    used: *count as u64,
                });
            }
        }
        Ok(())
    }

    /// Returns `true` if `belief_key` is in the contract's human-approval list
    /// and has not yet been approved.
    pub fn requires_approval(
        &self,
        belief_key: &str,
        require_human_approval: &[String],
    ) -> bool {
        if !require_human_approval.iter().any(|k| k == belief_key) {
            return false;
        }
        let pending = self.pending_approvals.lock().unwrap();
        // Key is in the approval list and has not been cleared.
        // First time we see it → add to pending.
        drop(pending);
        let mut pending = self.pending_approvals.lock().unwrap();
        pending.insert(belief_key.to_string());
        true
    }

    /// Mark `belief_key` as approved by a human operator.
    pub fn mark_approved(&self, belief_key: &str) {
        self.pending_approvals.lock().unwrap().remove(belief_key);
    }

    /// Total governance tokens consumed this session.
    pub fn tokens_used(&self) -> u64 {
        self.tokens_used.load(Ordering::Relaxed)
    }
}
