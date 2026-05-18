//! # epica-contracts
//!
//! Behavioral contracts `C = (P, I, G, R)` and Mnemonic Sovereignty nine primitives.
//!
//! Based on:
//! - Agent Behavioral Contracts (arXiv:2602.22302)
//! - Mnemonic Sovereignty (arXiv:2604.16548)
//!
//! **Phase 3** — fully implemented.
//!
//! ## Quick start: an allowlist write-auth policy
//!
//! ```
//! use epica_contracts::{AllowedAgents, AuthPolicy, MnemonicSovereignty};
//!
//! let mut sov = MnemonicSovereignty::permissive();
//! sov.write_auth = AuthPolicy {
//!     allowed_agents: AllowedAgents::Allowlist(vec!["alice".into()]),
//!     require_audit: false,
//! };
//!
//! assert!(sov.check_write_auth("alice").is_ok());
//! assert!(sov.check_write_auth("eve").is_err(), "non-listed agent must be rejected");
//! ```

pub mod config;
pub mod contract;
pub mod drift;
pub mod error;
pub mod governance;
pub mod invariant;
pub mod predicate;
pub mod recovery;
pub mod sovereignty;

pub use config::{ContractConfig, InvariantConfig, PreconditionConfig};
pub use contract::BehavioralContract;
pub use drift::DriftBound;
pub use error::ContractError;
pub use governance::GovernancePolicies;
pub use invariant::{ContractViolation, SessionInvariant, ViolationClass};
pub use predicate::{BeliefExists, BeliefPredicate, MinConfidence};
pub use recovery::RecoveryPolicy;
pub use sovereignty::{
    AuditDestination, AuditEntry, AuditEventType, AuditFormat, AuditMode, AuditPolicy,
    AllowedAgents, AuthPolicy,
    CrossAgentPolicy, SharingMode,
    default_causal_verify_fn, ForgetPolicy, ForgetTrigger, ForgetVerificationResult,
    MnemonicSovereignty, RecoveryVerificationResult, RecoveryVerifier,
    RetentionPolicy,
};
