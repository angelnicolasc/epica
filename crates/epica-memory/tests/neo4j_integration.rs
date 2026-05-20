//! End-to-end smoke test for the Neo4j long-term memory backend.
//!
//! ## Why `#[ignore]`?
//!
//! These tests require a live Neo4j instance. The default `cargo test` run is
//! offline; the CI workflow `.github/workflows/neo4j-integration.yml` opts in
//! by setting `EPICA_NEO4J_URI` (and friends) and passing `-- --ignored`.
//!
//! ## Running locally
//!
//! ```powershell
//! docker run --rm -d --name epica-neo4j -p 7687:7687 -p 7474:7474 `
//!     -e NEO4J_AUTH=neo4j/test_password `
//!     neo4j:5.18
//! # wait ~10s for the Bolt port to accept connections
//! $env:EPICA_NEO4J_URI  = "bolt://127.0.0.1:7687"
//! $env:EPICA_NEO4J_USER = "neo4j"
//! $env:EPICA_NEO4J_PASS = "test_password"
//! cargo test -p epica-memory --features neo4j --test neo4j_integration -- --ignored
//! ```
//!
//! ## What this covers
//!
//! - `Neo4jMemoryStore::connect` opens a Bolt session (driver compatibility check).
//! - `flush` `MERGE`s N beliefs into `(:Belief { key, ... })` nodes.
//! - `load` returns those beliefs decoded back into `BeliefNode` shape.
//! - The retention gate from `MnemonicSovereignty` drops expired nodes on
//!   load — exercised by backdating one row past the configured horizon.
//!
//! Not covered (out of scope, tracked as TD-P8-005): provenance and
//! `created_at_ms` round-trip — these are placeholders today because
//! `BeliefSnapshot` does not carry them.

#![cfg(feature = "neo4j")]

use std::time::{SystemTime, UNIX_EPOCH};

use epica_contracts::{MnemonicSovereignty, RetentionPolicy};
use epica_core::{BeliefId, BeliefSnapshot, BeliefValue};
use epica_memory::neo4j::Neo4jMemoryStore;
use epica_memory::store::LongTermMemoryStore;
use epica_memory::types::{BeliefKeyDescriptor, SchemaDescriptor};

/// Read connection settings from env. The test is silently skipped (returns
/// `None`) when any are missing — this is the contract that lets the workflow
/// gate by setting the env, and lets local runs opt in by exporting.
fn env_config() -> Option<(String, String, String)> {
    let uri = std::env::var("EPICA_NEO4J_URI").ok()?;
    let user = std::env::var("EPICA_NEO4J_USER").unwrap_or_else(|_| "neo4j".into());
    let pass = std::env::var("EPICA_NEO4J_PASS").ok()?;
    Some((uri, user, pass))
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn schema_for(keys: &[&str]) -> SchemaDescriptor {
    SchemaDescriptor {
        domain: "epica_it::neo4j_integration".to_string(),
        version: "1".to_string(),
        belief_keys: keys
            .iter()
            .map(|k| BeliefKeyDescriptor {
                key: (*k).to_string(),
                value_type: "string".to_string(),
                source: "test".to_string(),
                has_ttl: false,
                has_prospective_index: false,
            })
            .collect(),
    }
}

fn snapshot(key: &str, text: &str, fast: f32) -> BeliefSnapshot {
    BeliefSnapshot {
        // `BeliefId` is a slotmap key with a `Default` "null" value. The
        // backend keys rows by `key`, not by id, so a placeholder is fine.
        id: BeliefId::default(),
        key: key.to_string(),
        value: BeliefValue::Asserted(text.to_string()),
        fast_confidence: fast,
        slow_confidence: None,
        version_at_snapshot: 1,
    }
}

#[tokio::test]
#[ignore]
async fn neo4j_flush_then_load_round_trip() {
    let Some((uri, user, pass)) = env_config() else {
        eprintln!(
            "skipping: EPICA_NEO4J_URI / EPICA_NEO4J_PASS not set. \
             Start a Neo4j 5.x container and export the env to run this test."
        );
        return;
    };

    let store = Neo4jMemoryStore::connect(&uri, &user, &pass)
        .await
        .expect("Neo4j connect should succeed with valid credentials");

    // Keys are namespaced with the test name to avoid collisions when
    // multiple test runs share a long-lived container.
    let snaps = vec![
        snapshot("epica_it::neo4j_flush_then_load::alpha", "first fact", 0.71),
        snapshot("epica_it::neo4j_flush_then_load::beta", "second fact", 0.83),
    ];

    let sov = MnemonicSovereignty::permissive();
    let flush = store.flush(snaps, &sov).await.expect("flush should succeed");
    assert_eq!(flush.beliefs_written, 2);

    let schema = schema_for(&[
        "epica_it::neo4j_flush_then_load::alpha",
        "epica_it::neo4j_flush_then_load::beta",
    ]);
    let loaded = store
        .load(&schema, /*recency_decay=*/ 1.0)
        .await
        .expect("load should succeed");

    // Neo4j is a graph store, not an ordered table; sort before asserting.
    let mut keys: Vec<&str> = loaded.iter().map(|n| n.key.as_str()).collect();
    keys.sort();
    assert_eq!(
        keys,
        vec![
            "epica_it::neo4j_flush_then_load::alpha",
            "epica_it::neo4j_flush_then_load::beta",
        ]
    );

    // Values round-trip.
    for node in &loaded {
        let expected = if node.key.ends_with("alpha") { "first fact" } else { "second fact" };
        let BeliefValue::Asserted(text) = &node.value else {
            panic!("expected Asserted value, got {:?}", node.value);
        };
        assert_eq!(text, expected);
    }
}

#[tokio::test]
#[ignore]
async fn neo4j_load_respects_sovereignty_retention() {
    let Some((uri, user, pass)) = env_config() else {
        eprintln!("skipping: EPICA_NEO4J_URI not set");
        return;
    };

    // Build a sovereignty with a 1-hour retention window. We flush a row,
    // backdate its `updated_at_ms` ~2 hours into the past via a direct
    // Cypher write, and expect `load` to drop it.
    // `MnemonicSovereignty` is intentionally not `Clone` (it owns a closure
    // for recovery verification). Build two independent instances with the
    // same retention setting — one for the store, one for the flush call.
    let one_hour_ms: u64 = 60 * 60 * 1000;
    let make_sov = || {
        let mut s = MnemonicSovereignty::permissive();
        s.retention = RetentionPolicy::Duration(one_hour_ms);
        s
    };

    let store = Neo4jMemoryStore::connect_with_sovereignty(&uri, &user, &pass, make_sov())
        .await
        .expect("Neo4j connect should succeed");

    let key = "epica_it::neo4j_retention::should_expire";
    store
        .flush(vec![snapshot(key, "aged-out belief", 0.5)], &make_sov())
        .await
        .expect("flush should succeed");

    // Backdate the row through a direct Cypher write using the same
    // connection params. Anything older than `now - retention` should be
    // dropped on load.
    {
        use neo4rs::{query, ConfigBuilder, Graph};
        let cfg = ConfigBuilder::default()
            .uri(&uri)
            .user(&user)
            .password(&pass)
            .build()
            .expect("config builder");
        let graph = Graph::connect(cfg).await.expect("direct graph connect");
        let backdated = (now_ms() as i64) - (2 * one_hour_ms as i64);
        graph
            .run(
                query("MATCH (b:Belief {key: $key}) SET b.updated_at_ms = $ts")
                    .param("key", key)
                    .param("ts", backdated),
            )
            .await
            .expect("backdate update should succeed");
    }

    let schema = schema_for(&[key]);
    let loaded = store.load(&schema, 1.0).await.expect("load should succeed");
    assert!(
        loaded.is_empty(),
        "retention-expired belief must not be returned by load(); got {} rows",
        loaded.len()
    );
}
