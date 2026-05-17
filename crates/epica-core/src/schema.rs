//! Schema descriptor types for MCP Server Card generation.
//!
//! `SchemaDescriptor` is produced by `#[derive(BeliefState)]` via the
//! `schema_descriptor()` generated method and can be serialised as a
//! `.well-known/epica-server-card.json` entry by `epica-mcp` (Phase 5).

use serde::{Deserialize, Serialize};

/// Machine-readable description of a `BeliefState` struct.
///
/// Generated at compile time by `#[derive(BeliefState)]::schema_descriptor()`.
/// Intended to be serialised as part of an MCP Server Card so that clients can
/// discover the belief schema without connecting to the runtime.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaDescriptor {
    /// Rust type name of the annotated struct (e.g. `"CodebaseBeliefs"`).
    pub name: String,
    /// One descriptor per `#[belief(...)]`-annotated field.
    pub beliefs: Vec<BeliefFieldDescriptor>,
}

/// Compile-time metadata for a single belief field.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BeliefFieldDescriptor {
    /// Belief key — the field name as a string.
    pub key: String,
    /// Provenance source hint from the `source` attribute.
    pub source: String,
    /// Initial `fast_confidence` written by `to_belief_quad()`.
    pub initial_confidence: f32,
    /// Optional TTL in milliseconds (from `decays_after` attribute).
    pub ttl_ms: Option<u64>,
    /// Whether write-time prospective indexing is enabled for this field.
    pub prospect: bool,
    /// Per-node System 2 divergence threshold τ.
    pub reflection_threshold: f32,
    /// Field name of the causal parent, if any (from `causal_parent` attribute).
    pub causal_parent: Option<String>,
    /// Governance policy hint (e.g. `"human_approval"`).
    pub governance: Option<String>,
    /// Audit policy hint (e.g. `"full"`, `"violations_only"`).
    pub audit: Option<String>,
    /// Cache affinity hint (e.g. `"session"`, `"persistent"`, `"ephemeral"`).
    pub cache_affinity: Option<String>,
}
