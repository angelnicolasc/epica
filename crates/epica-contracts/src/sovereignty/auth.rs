//! Authorization policies for memory governance.
//!
//! Phase 3 implementation.

#![allow(dead_code)]

/// Authorization policy for a memory governance primitive.
#[derive(Debug, Clone)]
pub struct AuthPolicy {
    pub allowed_agents: AllowedAgents,
    pub require_audit: bool,
}

#[derive(Debug, Clone)]
pub enum AllowedAgents {
    /// Any agent in the system may perform the operation.
    Any,
    /// Only the listed agent IDs.
    Allowlist(Vec<String>),
    /// Any agent except the listed IDs.
    Denylist(Vec<String>),
    /// Requires explicit human approval via the governance gate.
    HumanApprovalRequired,
}

impl Default for AuthPolicy {
    fn default() -> Self {
        Self {
            allowed_agents: AllowedAgents::Any,
            require_audit: false,
        }
    }
}

impl AuthPolicy {
    /// Returns `true` if `agent_id` is permitted to perform this operation.
    pub fn allows(&self, agent_id: &str) -> bool {
        match &self.allowed_agents {
            AllowedAgents::Any => true,
            AllowedAgents::Allowlist(ids) => ids.iter().any(|id| id == agent_id),
            AllowedAgents::Denylist(ids) => !ids.iter().any(|id| id == agent_id),
            AllowedAgents::HumanApprovalRequired => false,
        }
    }

    /// Returns `true` if this policy requires explicit human approval before proceeding.
    pub fn requires_human_approval(&self) -> bool {
        matches!(self.allowed_agents, AllowedAgents::HumanApprovalRequired)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn any_allows_all() {
        let p = AuthPolicy { allowed_agents: AllowedAgents::Any, require_audit: false };
        assert!(p.allows("alice"));
        assert!(p.allows("bob"));
    }

    #[test]
    fn allowlist_blocks_unknown() {
        let p = AuthPolicy {
            allowed_agents: AllowedAgents::Allowlist(vec!["alice".into()]),
            require_audit: false,
        };
        assert!(p.allows("alice"));
        assert!(!p.allows("bob"));
    }

    #[test]
    fn denylist_blocks_listed() {
        let p = AuthPolicy {
            allowed_agents: AllowedAgents::Denylist(vec!["bob".into()]),
            require_audit: false,
        };
        assert!(p.allows("alice"));
        assert!(!p.allows("bob"));
    }

    #[test]
    fn human_approval_required_blocks_all() {
        let p = AuthPolicy {
            allowed_agents: AllowedAgents::HumanApprovalRequired,
            require_audit: false,
        };
        assert!(!p.allows("alice"));
        assert!(p.requires_human_approval());
    }
}
