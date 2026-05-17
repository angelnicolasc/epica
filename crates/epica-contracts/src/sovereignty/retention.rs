//! Retention policies for belief lifetime management.

#![allow(dead_code)]

/// How long a belief is retained before automatic expiry or review.
#[derive(Debug, Clone)]
pub enum RetentionPolicy {
    /// Retain indefinitely (default for deterministic facts).
    Permanent,
    /// Retain for a fixed duration in milliseconds.
    Duration(u64),
    /// Retain until a condition is met in the quad.
    Until { condition: String },
    /// Retain for the duration of the current session only.
    SessionScoped,
}

impl RetentionPolicy {
    /// Returns `true` if the belief should be evicted given its creation timestamp and now.
    ///
    /// `SessionScoped` and `Until` expiry are handled at session-end by the runtime,
    /// not on individual belief reads, so they return `false` here.
    pub fn is_expired(&self, created_at_ms: u64, now_ms: u64) -> bool {
        match self {
            RetentionPolicy::Permanent => false,
            RetentionPolicy::Duration(ms) => now_ms.saturating_sub(created_at_ms) > *ms,
            RetentionPolicy::SessionScoped => false,
            RetentionPolicy::Until { .. } => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permanent_never_expires() {
        assert!(!RetentionPolicy::Permanent.is_expired(0, u64::MAX));
    }

    #[test]
    fn duration_expires_after_ttl() {
        assert!(RetentionPolicy::Duration(1000).is_expired(0, 2000));
        assert!(!RetentionPolicy::Duration(1000).is_expired(0, 500));
    }

    #[test]
    fn session_scoped_never_expires_via_is_expired() {
        assert!(!RetentionPolicy::SessionScoped.is_expired(0, u64::MAX));
    }
}
