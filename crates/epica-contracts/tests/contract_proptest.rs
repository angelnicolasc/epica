//! Property-based tests for the Mnemonic Sovereignty authorization primitives.
//!
//! The unit-test suite in `sovereignty.rs` covers handpicked cases. The
//! properties here assert *global* invariants of the auth model that must
//! hold for ANY string identifiers and policy shape:
//!
//! - Allowlist semantics: a write succeeds iff the agent is in the list.
//! - Denylist semantics: a write succeeds iff the agent is NOT in the list.
//! - HumanApprovalRequired: never accepts an automated agent.
//! - Permissive: always accepts.
//!
//! Property tests catch regressions where, e.g., a normalization rule
//! (case, whitespace) is added in one decision path but not the other.

use epica_contracts::{AllowedAgents, AuthPolicy, MnemonicSovereignty};
use proptest::prelude::*;

fn sov_with_write(policy: AuthPolicy) -> MnemonicSovereignty {
    let mut sov = MnemonicSovereignty::permissive();
    sov.write_auth = policy;
    sov
}

proptest! {
    #[test]
    fn allowlist_admits_exactly_listed_agents(
        listed in proptest::collection::vec("[a-zA-Z0-9_]{1,16}", 1..8),
        candidate in "[a-zA-Z0-9_]{1,16}",
    ) {
        let policy = AuthPolicy {
            allowed_agents: AllowedAgents::Allowlist(listed.clone()),
            require_audit: false,
        };
        let sov = sov_with_write(policy);

        let should_pass = listed.iter().any(|a| a == &candidate);
        let actually_passes = sov.check_write_auth(&candidate).is_ok();

        prop_assert_eq!(
            should_pass,
            actually_passes,
            "Allowlist must admit exactly the listed agents — candidate={:?}, listed={:?}",
            candidate,
            listed
        );
    }

    #[test]
    fn denylist_rejects_exactly_listed_agents(
        listed in proptest::collection::vec("[a-zA-Z0-9_]{1,16}", 1..8),
        candidate in "[a-zA-Z0-9_]{1,16}",
    ) {
        let policy = AuthPolicy {
            allowed_agents: AllowedAgents::Denylist(listed.clone()),
            require_audit: false,
        };
        let sov = sov_with_write(policy);

        let should_fail = listed.iter().any(|a| a == &candidate);
        let actually_fails = sov.check_write_auth(&candidate).is_err();

        prop_assert_eq!(
            should_fail,
            actually_fails,
            "Denylist must reject exactly the listed agents — candidate={:?}, listed={:?}",
            candidate,
            listed
        );
    }

    #[test]
    fn human_approval_required_never_admits_automated_agents(
        candidate in "[a-zA-Z0-9_]{1,16}",
    ) {
        let policy = AuthPolicy {
            allowed_agents: AllowedAgents::HumanApprovalRequired,
            require_audit: false,
        };
        let sov = sov_with_write(policy);
        prop_assert!(
            sov.check_write_auth(&candidate).is_err(),
            "HumanApprovalRequired must reject ALL automated agents"
        );
    }

    #[test]
    fn permissive_admits_any_agent(candidate in "[a-zA-Z0-9_]{1,16}") {
        let sov = MnemonicSovereignty::permissive();
        prop_assert!(
            sov.check_write_auth(&candidate).is_ok(),
            "permissive() must admit every agent"
        );
    }

    /// The decision must be *deterministic*: calling check_write_auth twice
    /// with the same input always produces the same result.
    #[test]
    fn auth_decision_is_deterministic(
        listed in proptest::collection::vec("[a-zA-Z0-9_]{1,16}", 1..8),
        candidate in "[a-zA-Z0-9_]{1,16}",
    ) {
        let policy = AuthPolicy {
            allowed_agents: AllowedAgents::Allowlist(listed),
            require_audit: false,
        };
        let sov = sov_with_write(policy);
        let first = sov.check_write_auth(&candidate).is_ok();
        let second = sov.check_write_auth(&candidate).is_ok();
        prop_assert_eq!(first, second, "check_write_auth must be deterministic");
    }
}
