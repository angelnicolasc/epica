//! TOML-deserializable contract configuration.
//!
//! Solves the trait-object serialization problem: `Box<dyn BeliefPredicate>` is not
//! `Deserialize`. `ContractConfig` uses plain enum variants that serde can handle,
//! and `BehavioralContract::from_config()` instantiates the real predicates.
//!
//! ## Example TOML
//! ```toml
//! domain = "deployment_safety"
//! alpha  = 0.05
//! gamma  = 0.5
//!
//! [[preconditions]]
//! type           = "min_confidence"
//! key            = "environment"
//! min_confidence = 0.8
//!
//! [[preconditions]]
//! type = "exists"
//! key  = "user_goal"
//!
//! [[invariants]]
//! key            = "environment"
//! min_confidence = 0.5
//! severity       = "hard"
//!
//! [governance]
//! max_tokens_per_session = 50000
//! ```

use serde::{Deserialize, Serialize};

use crate::{
    contract::BehavioralContract,
    governance::GovernancePolicies,
    invariant::{SessionInvariant, ViolationClass},
    predicate::{BeliefExists, MinConfidence},
    recovery::RecoveryPolicy,
};

/// Top-level contract configuration, loadable from TOML.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractConfig {
    /// Domain / name of the contract.
    pub domain: String,

    /// Natural drift rate `α` (default 0.05). Tune per deployment.
    #[serde(default = "default_alpha")]
    pub alpha: f64,

    /// Enforcement rate `γ` (default 0.5). Higher = stricter.
    #[serde(default = "default_gamma")]
    pub gamma: f64,

    /// Probabilistic satisfaction probability `p` (default 0.99).
    #[serde(default = "default_p")]
    pub p: f64,

    /// Maximum allowed violations in a window of `k` steps (default 1).
    #[serde(default = "default_delta")]
    pub delta: u32,

    /// Window size in steps (default 100).
    #[serde(default = "default_k")]
    pub k: u32,

    /// Preconditions: must hold before any action.
    #[serde(default)]
    pub preconditions: Vec<PreconditionConfig>,

    /// Invariants: must hold at every `update_belief()` call.
    #[serde(default)]
    pub invariants: Vec<InvariantConfig>,

    /// Governance limits and audit policy.
    #[serde(default)]
    pub governance: GovernancePolicies,
}

/// A serializable representation of a belief precondition.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PreconditionConfig {
    /// Belief `key` must exist with `fast_confidence >= min_confidence`.
    MinConfidence {
        key: String,
        #[serde(default = "default_min_confidence")]
        min_confidence: f32,
    },
    /// Belief `key` must exist (any confidence).
    Exists { key: String },
}

/// A serializable representation of a session invariant.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvariantConfig {
    /// The belief key this invariant monitors.
    pub key: String,

    /// Minimum `fast_confidence` required (default 0.5).
    #[serde(default = "default_min_confidence")]
    pub min_confidence: f32,

    /// Violation severity: `"soft"` | `"hard"` (default) | `"critical"`.
    #[serde(default = "default_severity")]
    pub severity: String,

    /// Steps between violation detection and recovery (default 5, from StepShield 2026).
    #[serde(default = "default_gap")]
    pub max_intervention_gap_steps: u32,
}

// ── serde default helpers ─────────────────────────────────────────────────────

fn default_alpha() -> f64 { 0.05 }
fn default_gamma() -> f64 { 0.5 }
fn default_p() -> f64 { 0.99 }
fn default_delta() -> u32 { 1 }
fn default_k() -> u32 { 100 }
fn default_min_confidence() -> f32 { 0.5 }
fn default_severity() -> String { "hard".to_string() }
fn default_gap() -> u32 { 5 }

// ── BehavioralContract::from_config ──────────────────────────────────────────

impl BehavioralContract {
    /// Build a `BehavioralContract` from a TOML-deserialized `ContractConfig`.
    ///
    /// This is the bridge between the serializable config representation and the
    /// runtime contract, which holds `Box<dyn BeliefPredicate>` trait objects.
    pub fn from_config(cfg: ContractConfig) -> Self {
        let preconditions = cfg
            .preconditions
            .into_iter()
            .map(|p| -> Box<dyn crate::predicate::BeliefPredicate> {
                match p {
                    PreconditionConfig::MinConfidence { key, min_confidence } => {
                        Box::new(MinConfidence { key, threshold: min_confidence })
                    }
                    PreconditionConfig::Exists { key } => {
                        Box::new(BeliefExists { key })
                    }
                }
            })
            .collect();

        let invariants = cfg
            .invariants
            .into_iter()
            .map(|inv| {
                let class = match inv.severity.as_str() {
                    "soft" => ViolationClass::Soft,
                    "critical" => ViolationClass::Critical,
                    _ => ViolationClass::Hard,
                };
                SessionInvariant {
                    predicate: Box::new(MinConfidence {
                        key: inv.key,
                        threshold: inv.min_confidence,
                    }),
                    class,
                    max_intervention_gap_steps: inv.max_intervention_gap_steps,
                }
            })
            .collect();

        BehavioralContract {
            domain: cfg.domain,
            preconditions,
            invariants,
            governance: cfg.governance,
            recovery: RecoveryPolicy::RollbackToLastCheckpoint,
            p: cfg.p,
            delta: cfg.delta,
            k: cfg.k,
            alpha: cfg.alpha,
            gamma: cfg.gamma,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL_TOML: &str = r#"
domain = "test_contract"
"#;

    const FULL_TOML: &str = r#"
domain = "deployment_safety"
alpha  = 0.03
gamma  = 0.6

[[preconditions]]
type           = "min_confidence"
key            = "environment"
min_confidence = 0.8

[[preconditions]]
type = "exists"
key  = "user_goal"

[[invariants]]
key            = "environment"
min_confidence = 0.5
severity       = "hard"

[[invariants]]
key            = "user_goal"
min_confidence = 0.3
severity       = "soft"

[governance]
max_tokens_per_session = 50000
"#;

    #[test]
    fn minimal_toml_deserializes_with_defaults() {
        let cfg: ContractConfig = toml::from_str(MINIMAL_TOML).unwrap();
        assert_eq!(cfg.domain, "test_contract");
        assert!((cfg.alpha - 0.05).abs() < 1e-10);
        assert!((cfg.gamma - 0.5).abs() < 1e-10);
        assert!(cfg.preconditions.is_empty());
        assert!(cfg.invariants.is_empty());
    }

    #[test]
    fn full_toml_deserializes_all_fields() {
        let cfg: ContractConfig = toml::from_str(FULL_TOML).unwrap();
        assert_eq!(cfg.domain, "deployment_safety");
        assert_eq!(cfg.preconditions.len(), 2);
        assert_eq!(cfg.invariants.len(), 2);
        assert_eq!(cfg.governance.max_tokens_per_session, Some(50_000));
    }

    #[test]
    fn from_config_builds_working_contract() {
        let cfg: ContractConfig = toml::from_str(FULL_TOML).unwrap();
        let contract = BehavioralContract::from_config(cfg);
        assert_eq!(contract.domain, "deployment_safety");
        assert_eq!(contract.preconditions.len(), 2);
        assert_eq!(contract.invariants.len(), 2);
        assert!((contract.alpha - 0.03).abs() < 1e-10);
        assert!((contract.gamma - 0.6).abs() < 1e-10);
    }

    #[test]
    fn contract_evaluates_preconditions_correctly() {
        use epica_core::{BeliefNode, BeliefQuad, BeliefValue, Provenance};

        let cfg: ContractConfig = toml::from_str(FULL_TOML).unwrap();
        let contract = BehavioralContract::from_config(cfg);
        let mut quad = BeliefQuad::new();

        // Neither key present → both preconditions fail
        assert!(!contract.check_preconditions(&quad));

        // Insert environment with high confidence
        quad.insert(BeliefNode::new(
            "environment",
            BeliefValue::Asserted("staging".into()),
            Provenance::UserStatement { turn: 0 },
            0.9,
        ));
        // user_goal still missing → exists precondition fails
        assert!(!contract.check_preconditions(&quad));

        // Insert user_goal (exists check — any confidence)
        quad.insert(BeliefNode::new(
            "user_goal",
            BeliefValue::Asserted("deploy".into()),
            Provenance::UserStatement { turn: 0 },
            0.1,
        ));
        // Both preconditions now satisfied
        assert!(contract.check_preconditions(&quad));
    }

    #[test]
    fn contract_evaluates_invariants_correctly() {
        use epica_core::{BeliefNode, BeliefQuad, BeliefValue, Provenance};

        let cfg: ContractConfig = toml::from_str(FULL_TOML).unwrap();
        let contract = BehavioralContract::from_config(cfg);
        let mut quad = BeliefQuad::new();

        // environment missing → hard invariant violated
        assert!(contract.check_invariants(&quad, None).is_some());

        quad.insert(BeliefNode::new(
            "environment",
            BeliefValue::Asserted("staging".into()),
            Provenance::UserStatement { turn: 0 },
            0.6,
        ));
        // environment >= 0.5 → hard invariant satisfied (soft user_goal skipped)
        assert!(contract.check_invariants(&quad, None).is_none());

        // Drop environment confidence below threshold
        quad.insert(BeliefNode::new(
            "environment",
            BeliefValue::Asserted("unknown".into()),
            Provenance::UserStatement { turn: 0 },
            0.1,
        ));
        assert!(contract.check_invariants(&quad, None).is_some());
    }

    #[test]
    fn drift_bound_correct() {
        let cfg: ContractConfig = toml::from_str(FULL_TOML).unwrap();
        let contract = BehavioralContract::from_config(cfg);
        let db = contract.drift_bound();
        assert!((db.expected_drift - 0.03 / 0.6).abs() < 1e-10);
    }

    #[test]
    fn precondition_config_min_confidence_variant() {
        let toml_str = r#"
domain = "x"
[[preconditions]]
type = "min_confidence"
key  = "foo"
"#;
        let cfg: ContractConfig = toml::from_str(toml_str).unwrap();
        match &cfg.preconditions[0] {
            PreconditionConfig::MinConfidence { key, min_confidence } => {
                assert_eq!(key, "foo");
                assert!((min_confidence - 0.5).abs() < 1e-6);
            }
            _ => panic!("wrong variant"),
        }
    }
}
