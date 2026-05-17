//! # epica-memory
//!
//! `LongTermMemoryStore` trait and reference implementations.
//!
//! Dual-store architecture inspired by Kumiho (arXiv:2603.17244):
//! - `RedisMemoryStore`: active working memory (warm beliefs from last run)
//! - `Neo4jMemoryStore`: long-term causal belief graph (across runs)
//!
//! **Phase 7** — trait defined; implementations are `todo!()`.

#![allow(missing_docs)]

#[cfg(feature = "neo4j")]
pub mod neo4j;
pub mod store;
pub mod types;

#[cfg(feature = "redis")]
pub mod redis;

pub use store::{LongTermMemoryStore, MemoryError};
pub use types::{FlushResult, SchemaDescriptor};
