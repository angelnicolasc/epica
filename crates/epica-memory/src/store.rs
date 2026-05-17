//! LongTermMemoryStore trait.
//!
//! Phase 7 implementation.

#![allow(dead_code, unused_variables)]

use async_trait::async_trait;
use epica_core::{BeliefNode, BeliefSnapshot};

use crate::types::{FlushResult, SchemaDescriptor};

/// Abstraction over persistent memory backends.
///
/// Implemented by `RedisMemoryStore` (working memory) and `Neo4jMemoryStore` (graph long-term).
/// The trait abstraction means Epica does not impose a specific backend.
///
/// The nine primitives of `MnemonicSovereignty` are enforced in `flush()` and `load()`.
#[async_trait]
pub trait LongTermMemoryStore: Send + Sync {
    /// Flush the current run's beliefs to long-term storage.
    ///
    /// `causal_subgraph` preserves the causal structure, not just the values.
    /// `sovereignty` enforces the nine governance primitives during flush.
    async fn flush(
        &self,
        beliefs: Vec<BeliefSnapshot>,
        sovereignty: &epica_contracts::MnemonicSovereignty,
    ) -> Result<FlushResult, MemoryError>;

    /// Load beliefs from long-term storage for a new run.
    ///
    /// `recency_decay` reduces the `fast_confidence` of loaded beliefs:
    /// a belief that was true 3 days ago has less epistemic authority than
    /// a fresh tool result.
    async fn load(
        &self,
        domain_schema: &SchemaDescriptor,
        recency_decay: f32,
    ) -> Result<Vec<BeliefNode>, MemoryError>;

    /// Access the sovereignty policy for this store.
    fn sovereignty(&self) -> &epica_contracts::MnemonicSovereignty;
}

/// Errors from memory store operations.
#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    #[error("connection error: {0}")]
    Connection(String),

    #[error("serialization error: {0}")]
    Serialization(String),

    #[error("sovereignty violation: {0}")]
    Sovereignty(String),
}
