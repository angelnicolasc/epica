//! Redis implementation of `LongTermMemoryStore` (working memory).
//!
//! Redis is used for the *active* working memory BeliefQuad — beliefs from the
//! previous run that are still warm. Neo4j holds the long-term causal graph.

#![cfg(feature = "redis")]

use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use epica_core::{BeliefNode, BeliefSnapshot};
use redis::{aio::ConnectionManager, AsyncCommands, Client};

use crate::{
    store::{LongTermMemoryStore, MemoryError},
    types::{FlushResult, SchemaDescriptor},
};

const KEY_PREFIX: &str = "epica:belief:";

/// Redis-backed working memory store.
pub struct RedisMemoryStore {
    manager: ConnectionManager,
    sovereignty: epica_contracts::MnemonicSovereignty,
}

impl RedisMemoryStore {
    pub async fn connect(url: &str) -> Result<Self, MemoryError> {
        let client = Client::open(url)
            .map_err(|e| MemoryError::Connection(e.to_string()))?;
        let manager = ConnectionManager::new(client)
            .await
            .map_err(|e| MemoryError::Connection(e.to_string()))?;
        Ok(Self {
            manager,
            sovereignty: epica_contracts::MnemonicSovereignty::permissive(),
        })
    }

    pub async fn connect_with_sovereignty(
        url: &str,
        sovereignty: epica_contracts::MnemonicSovereignty,
    ) -> Result<Self, MemoryError> {
        let mut store = Self::connect(url).await?;
        store.sovereignty = sovereignty;
        Ok(store)
    }
}

#[async_trait]
impl LongTermMemoryStore for RedisMemoryStore {
    async fn flush(
        &self,
        beliefs: Vec<BeliefSnapshot>,
        sovereignty: &epica_contracts::MnemonicSovereignty,
    ) -> Result<FlushResult, MemoryError> {
        let start_ms = now_ms();
        let mut conn = self.manager.clone();
        let mut written = 0usize;

        // Determine TTL from sovereignty retention policy
        let ttl_seconds: Option<u64> = match &sovereignty.retention {
            epica_contracts::RetentionPolicy::Permanent => None,
            epica_contracts::RetentionPolicy::Duration(ms) => Some(ms / 1000),
            epica_contracts::RetentionPolicy::SessionScoped => Some(3600), // 1h default session
            epica_contracts::RetentionPolicy::Until { .. } => None, // condition-based; no Redis TTL
        };

        for snapshot in &beliefs {
            let key = format!("{KEY_PREFIX}{}", snapshot.key);
            let json = serde_json::to_string(snapshot)
                .map_err(|e| MemoryError::Serialization(e.to_string()))?;

            if let Some(ttl) = ttl_seconds {
                let _: () = conn
                    .set_ex(&key, &json, ttl)
                    .await
                    .map_err(|e| MemoryError::Connection(e.to_string()))?;
            } else {
                let _: () = conn
                    .set(&key, &json)
                    .await
                    .map_err(|e| MemoryError::Connection(e.to_string()))?;
            }
            written += 1;
        }

        let duration_ms = now_ms() - start_ms;
        Ok(FlushResult {
            beliefs_written: written,
            causal_edges_written: 0, // Redis stores flat beliefs; causal graph → Neo4j
            duration_ms,
        })
    }

    async fn load(
        &self,
        _domain_schema: &SchemaDescriptor,
        recency_decay: f32,
    ) -> Result<Vec<BeliefNode>, MemoryError> {
        let mut conn = self.manager.clone();

        // SCAN for all belief keys
        let pattern = format!("{KEY_PREFIX}*");
        let keys: Vec<String> = redis::cmd("KEYS")
            .arg(&pattern)
            .query_async(&mut conn)
            .await
            .map_err(|e| MemoryError::Connection(e.to_string()))?;

        let mut nodes = Vec::with_capacity(keys.len());
        for key in keys {
            let json: Option<String> = conn
                .get(&key)
                .await
                .map_err(|e| MemoryError::Connection(e.to_string()))?;

            if let Some(json) = json {
                let snapshot: BeliefSnapshot = serde_json::from_str(&json)
                    .map_err(|e| MemoryError::Serialization(e.to_string()))?;

                // Reconstruct BeliefNode from snapshot, applying recency decay
                let mut node = BeliefNode::new(
                    &snapshot.key,
                    snapshot.value,
                    epica_core::Provenance::RuntimeInference {
                        derived_from: vec![],
                    },
                    (snapshot.fast_confidence * recency_decay).clamp(0.0, 1.0),
                );
                node.slow_confidence = snapshot.slow_confidence.map(|c| c * recency_decay);
                nodes.push(node);
            }
        }
        Ok(nodes)
    }

    fn sovereignty(&self) -> &epica_contracts::MnemonicSovereignty {
        &self.sovereignty
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
