//! Governance policies: resource limits and authorization.

/// Resource limits and authorization gates for a behavioral contract.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct GovernancePolicies {
    pub max_tokens_per_session: Option<u64>,
    pub max_tool_calls_per_belief: Option<u32>,
    /// Belief keys that require explicit human approval before the agent acts on them.
    #[serde(default)]
    pub require_human_approval: Vec<String>,
    #[serde(default)]
    pub audit_trail: AuditPolicy,
}

/// How to record contract-related events.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct AuditPolicy {
    pub mode: AuditMode,
}

/// Granularity of audit logging.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditMode {
    /// Every write generates an audit entry.
    Full,
    /// Only beliefs with `audit = "full"` in their attributes.
    #[default]
    Selective,
    /// Only when a contract is violated.
    ViolationsOnly,
}
