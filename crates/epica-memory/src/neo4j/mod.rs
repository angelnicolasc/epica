//! Neo4j implementation of `LongTermMemoryStore` (long-term causal graph).
//!
//! No official Rust Neo4j driver is available in this workspace.
//! All methods return `Err(MemoryError::Connection(...))` with a clear message.
//! See TD-NEW-001 in DEVLOG.md for the unblocking path.

use async_trait::async_trait;
use epica_core::{BeliefNode, BeliefSnapshot};

use crate::{
    store::{LongTermMemoryStore, MemoryError},
    types::{FlushResult, SchemaDescriptor},
};

const NOT_AVAILABLE: &str = "Neo4j driver not yet available — see TD-NEW-001";

/// Neo4j-backed long-term memory store.
///
/// Blocked on a stable Rust Neo4j driver (`neo4rs` or HTTP API transport).
/// See TD-NEW-001 in DEVLOG.md.
pub struct Neo4jMemoryStore {
    #[allow(dead_code)]
    uri: String,
    sovereignty: epica_contracts::MnemonicSovereignty,
}

impl Neo4jMemoryStore {
    pub async fn connect(_uri: &str, _user: &str, _password: &str) -> Result<Self, MemoryError> {
        Err(MemoryError::Connection(NOT_AVAILABLE.into()))
    }
}

#[async_trait]
impl LongTermMemoryStore for Neo4jMemoryStore {
    async fn flush(
        &self,
        _beliefs: Vec<BeliefSnapshot>,
        _sovereignty: &epica_contracts::MnemonicSovereignty,
    ) -> Result<FlushResult, MemoryError> {
        Err(MemoryError::Connection(NOT_AVAILABLE.into()))
    }

    async fn load(
        &self,
        _domain_schema: &SchemaDescriptor,
        _recency_decay: f32,
    ) -> Result<Vec<BeliefNode>, MemoryError> {
        Err(MemoryError::Connection(NOT_AVAILABLE.into()))
    }

    fn sovereignty(&self) -> &epica_contracts::MnemonicSovereignty {
        &self.sovereignty
    }
}
