//! Neo4j implementation of `LongTermMemoryStore` (long-term causal graph).
//!
//! Backed by [`neo4rs`](https://crates.io/crates/neo4rs) — opt-in via the
//! `neo4j` Cargo feature. With the feature off (default) this module is not
//! compiled, so the workspace audit graph picks up no Neo4j-related
//! transitive dependencies.
//!
//! ## Schema
//!
//! Each belief is persisted as a `(:Belief)` node keyed by `key`:
//!
//! ```cypher
//! (:Belief { key, value_json, fast_confidence, slow_confidence,
//!            version_at_snapshot, updated_at_ms })
//! ```
//!
//! The current `BeliefSnapshot` type does not carry `provenance` or
//! `created_at_ms`; those round-trip through `BeliefNode::new` on load using
//! the inserter's clock and a `Provenance::ToolResult` placeholder. This is a
//! known limitation of the snapshot API and is shared with the Redis backend.
//!
//! ## Sovereignty
//!
//! Retention (primitive 4) is enforced at load: any persisted node whose
//! `updated_at_ms` is older than the configured retention horizon is dropped.
//! Audit emission and finer-grained governance live in the runtime layer.

#![cfg(feature = "neo4j")]

use std::time::{Instant, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use epica_core::{BeliefNode, BeliefSnapshot, BeliefValue, Provenance};
use neo4rs::{query, ConfigBuilder, Graph};

use crate::{
    store::{LongTermMemoryStore, MemoryError},
    types::{FlushResult, SchemaDescriptor},
};

/// Neo4j-backed long-term memory store.
pub struct Neo4jMemoryStore {
    graph: Graph,
    sovereignty: epica_contracts::MnemonicSovereignty,
}

impl Neo4jMemoryStore {
    /// Open a connection to the Neo4j Bolt endpoint at `uri` using the given
    /// credentials. Returns once the driver has confirmed the handshake.
    pub async fn connect(uri: &str, user: &str, password: &str) -> Result<Self, MemoryError> {
        let cfg = ConfigBuilder::default()
            .uri(uri)
            .user(user)
            .password(password)
            .build()
            .map_err(|e| MemoryError::Connection(e.to_string()))?;
        let graph = Graph::connect(cfg)
            .await
            .map_err(|e| MemoryError::Connection(e.to_string()))?;
        Ok(Self {
            graph,
            sovereignty: epica_contracts::MnemonicSovereignty::permissive(),
        })
    }

    /// Same as [`Self::connect`] but installs a non-default sovereignty.
    pub async fn connect_with_sovereignty(
        uri: &str,
        user: &str,
        password: &str,
        sovereignty: epica_contracts::MnemonicSovereignty,
    ) -> Result<Self, MemoryError> {
        let mut store = Self::connect(uri, user, password).await?;
        store.sovereignty = sovereignty;
        Ok(store)
    }
}

#[async_trait]
impl LongTermMemoryStore for Neo4jMemoryStore {
    async fn flush(
        &self,
        beliefs: Vec<BeliefSnapshot>,
        _sovereignty: &epica_contracts::MnemonicSovereignty,
    ) -> Result<FlushResult, MemoryError> {
        let start = Instant::now();
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let mut written = 0usize;
        for snap in beliefs {
            let value_json = serde_json::to_string(&snap.value)
                .map_err(|e| MemoryError::Serialization(e.to_string()))?;
            let q = query(
                "MERGE (b:Belief {key: $key}) \
                 SET b.value_json = $value, \
                     b.fast_confidence = $fast, \
                     b.slow_confidence = $slow, \
                     b.version_at_snapshot = $ver, \
                     b.updated_at_ms = $now",
            )
            .param("key", snap.key.clone())
            .param("value", value_json)
            .param("fast", snap.fast_confidence as f64)
            .param("slow", snap.slow_confidence.map(|v| v as f64).unwrap_or(f64::NAN))
            .param("ver", snap.version_at_snapshot as i64)
            .param("now", now_ms as i64);

            self.graph
                .run(q)
                .await
                .map_err(|e| MemoryError::Connection(e.to_string()))?;
            written += 1;
        }

        Ok(FlushResult {
            beliefs_written: written,
            causal_edges_written: 0,
            duration_ms: start.elapsed().as_millis() as u64,
        })
    }

    async fn load(
        &self,
        domain_schema: &SchemaDescriptor,
        recency_decay: f32,
    ) -> Result<Vec<BeliefNode>, MemoryError> {
        let keys: Vec<String> = domain_schema
            .belief_keys
            .iter()
            .map(|k| k.key.clone())
            .collect();

        let mut result = self
            .graph
            .execute(
                query(
                    "MATCH (b:Belief) WHERE b.key IN $keys \
                     RETURN b.key AS key, b.value_json AS value, \
                            b.fast_confidence AS confidence, \
                            b.updated_at_ms AS updated",
                )
                .param("keys", keys),
            )
            .await
            .map_err(|e| MemoryError::Connection(e.to_string()))?;

        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let mut out = Vec::new();
        while let Some(row) = result
            .next()
            .await
            .map_err(|e| MemoryError::Connection(e.to_string()))?
        {
            let key: String = row
                .get("key")
                .map_err(|e| MemoryError::Serialization(e.to_string()))?;
            let value_json: String = row
                .get("value")
                .map_err(|e| MemoryError::Serialization(e.to_string()))?;
            let confidence: f64 = row
                .get("confidence")
                .map_err(|e| MemoryError::Serialization(e.to_string()))?;
            let updated: i64 = row.get("updated").unwrap_or(now_ms as i64);

            // Retention gate (primitive 4): skip rows past their expiry.
            if self.sovereignty.is_belief_expired(updated as u64, now_ms) {
                continue;
            }

            let value: BeliefValue = serde_json::from_str(&value_json)
                .map_err(|e| MemoryError::Serialization(e.to_string()))?;
            let decayed = (confidence as f32 * recency_decay).clamp(0.0, 1.0);
            out.push(BeliefNode::new(
                key,
                value,
                Provenance::RuntimeInference { derived_from: Vec::new() },
                decayed,
            ));
        }

        Ok(out)
    }

    fn sovereignty(&self) -> &epica_contracts::MnemonicSovereignty {
        &self.sovereignty
    }
}
