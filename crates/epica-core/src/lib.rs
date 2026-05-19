#![forbid(unsafe_code)]
#![warn(missing_docs, clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

//! # epica-core
//!
//! BeliefQuad: four orthogonal causal belief graphs over `SlotMap`, with
//! formal AGM belief revision (K\*2–K\*6), System 1 Noisy-OR propagation,
//! checkpointing, rollback, and structural diff.
//!
//! ## Architecture
//!
//! ```text
//! BeliefQuad
//! ├── nodes: SlotMap<BeliefId, BeliefNode>   ← single source of truth
//! ├── semantic:  SemanticGraph               ← concept similarity, contradiction
//! ├── temporal:  TemporalGraph               ← precedence, decay
//! ├── causal:    CausalGraph                 ← entailment, System 1, counterfactuals
//! ├── entity:    EntityGraph                 ← provenance, authorization
//! └── prospective_index: ProspectiveIndex    ← write-time scenario index (Phase 4)
//! ```
//!
//! ## Phase status
//!
//! This crate implements **Phase 1** of the Epica roadmap. See `docs/phase_roadmap.md`
//! and `DEVLOG.md` for what is real vs. stubbed in the full workspace.
//!
//! ## Quick start
//!
//! ```rust
//! use epica_core::{BeliefNode, BeliefQuad, BeliefValue, Provenance};
//!
//! let mut quad = BeliefQuad::new();
//!
//! // Insert a belief and verify it round-trips by id.
//! let id = quad.insert(BeliefNode::new(
//!     "user_intent",
//!     BeliefValue::Asserted("refactor auth".into()),
//!     Provenance::UserStatement { turn: 0 },
//!     0.9,
//! ));
//! assert_eq!(quad.get(id).unwrap().key, "user_intent");
//!
//! // Revise the belief (Levi identity: contract-then-expand on contradiction).
//! let record = quad.revise(
//!     id,
//!     BeliefValue::Asserted("ship feature X".into()),
//!     Provenance::UserStatement { turn: 1 },
//!     0.85,
//! ).expect("revision succeeds");
//! assert_eq!(record.belief_id, id);
//! assert!(record.postulate_audit.all_critical_pass(),
//!         "K*2-K*5 are hard errors in every build");
//! ```

pub mod belief;
pub mod checkpoint;
pub mod counterfactual;
pub mod diff;
pub mod embedding;
pub mod error;
pub mod prospective;
pub mod quad;
pub mod revision;
pub mod schema;
pub mod system1;

// ── Public re-exports ─────────────────────────────────────────────────────────

pub use belief::{
    BeliefId, BeliefNode, BeliefValue, Provenance,
    DEFAULT_REFLECTION_THRESHOLD, effective_confidence, should_activate_system2,
};

pub use checkpoint::{BeliefQuadCheckpoint, CheckpointId};

pub use counterfactual::CounterfactualWorld;

pub use diff::{BeliefModification, BeliefQuadDiff, BeliefSnapshot};

pub use embedding::{
    cosine_similarity, CachedEmbeddingProvider, EmbeddingProvider, EquivalenceVerdict,
    NullEmbeddingProvider, SemanticEquivalence,
};

pub use error::EpicaError;

pub use prospective::{CausalEvent, ProspectiveEntry, ProspectiveIndex};

pub use quad::{BeliefQuad, CausalEdge, EntityEdge, SemanticEdge, TemporalEdge, VerdictTrace};

pub use revision::{BeliefRevisionError, PostulateAudit, RevisionRecord, RollbackError};

pub use schema::{BeliefFieldDescriptor, SchemaDescriptor};
