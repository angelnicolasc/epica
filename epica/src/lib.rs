//! # epica
//!
//! Facade crate — re-exports from all Epica sub-crates based on enabled features.
//!
//! ```toml
//! [dependencies]
//! epica = { version = "0.1", features = ["full"] }
//! ```

#[cfg(feature = "core")]
pub use epica_core as core;

#[cfg(feature = "runtime")]
pub use epica_runtime as runtime;

#[cfg(feature = "contracts")]
pub use epica_contracts as contracts;

#[cfg(feature = "macros")]
pub use epica_macros as macros;

#[cfg(feature = "mcp")]
pub use epica_mcp as mcp;

#[cfg(feature = "memory")]
pub use epica_memory as memory;

// Convenience flat re-exports for the common case (features = ["core"])
#[cfg(feature = "core")]
pub use epica_core::{
    BeliefId, BeliefNode, BeliefQuad, BeliefValue, Provenance,
    CausalEdge, EntityEdge, SemanticEdge, TemporalEdge,
    CheckpointId, BeliefQuadDiff, RevisionRecord, EpicaError,
};
