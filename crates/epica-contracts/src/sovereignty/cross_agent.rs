//! Cross-agent belief propagation policies.

#![allow(dead_code)]

/// Governs which beliefs may be shared with other agents and under what conditions.
#[derive(Debug, Clone, Default)]
pub struct CrossAgentPolicy {
    pub sharing_mode: SharingMode,
    pub require_consent: bool,
}

#[derive(Debug, Clone, Default)]
pub enum SharingMode {
    /// Beliefs may not be shared with other agents.
    Isolated,
    /// Beliefs may be shared with explicitly listed agents.
    Allowlist(Vec<String>),
    /// Beliefs may be shared with all agents in the same session.
    #[default]
    SessionShared,
    /// Beliefs may be shared globally (e.g., system-wide knowledge base).
    Global,
}

impl CrossAgentPolicy {
    /// Returns `true` if this policy permits sharing a belief with `agent_id`.
    pub fn allows_share_with(&self, agent_id: &str) -> bool {
        match &self.sharing_mode {
            SharingMode::Isolated => false,
            SharingMode::Allowlist(ids) => ids.iter().any(|id| id == agent_id),
            SharingMode::SessionShared => true,
            SharingMode::Global => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isolated_blocks_all() {
        let p = CrossAgentPolicy { sharing_mode: SharingMode::Isolated, require_consent: false };
        assert!(!p.allows_share_with("anyone"));
    }

    #[test]
    fn allowlist_works() {
        let p = CrossAgentPolicy {
            sharing_mode: SharingMode::Allowlist(vec!["bob".into()]),
            require_consent: false,
        };
        assert!(p.allows_share_with("bob"));
        assert!(!p.allows_share_with("carol"));
    }

    #[test]
    fn session_shared_allows_all() {
        let p = CrossAgentPolicy { sharing_mode: SharingMode::SessionShared, require_consent: false };
        assert!(p.allows_share_with("anyone"));
    }
}
