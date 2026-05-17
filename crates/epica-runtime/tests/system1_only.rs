//! Integration tests: System 1 only path (no LLM client attached).

use epica_core::{BeliefNode, BeliefQuad, BeliefValue, Provenance};
use epica_runtime::{BeliefRuntime, RuntimeUpdateResult};
use serde_json::json;

fn rt() -> BeliefRuntime {
    BeliefRuntime::new(BeliefQuad::new(), 0.5, 10, 0.0)
}

fn asserted(key: &str, val: &str, conf: f32) -> BeliefNode {
    BeliefNode::new(
        key,
        BeliefValue::Asserted(val.into()),
        Provenance::UserStatement { turn: 0 },
        conf,
    )
}

// ── 1. insert_belief / get_by_key ─────────────────────────────────────────────

#[tokio::test]
async fn insert_and_get_by_key() {
    let rt = rt();
    let id = rt.insert_belief(asserted("intent", "refactor auth", 0.9)).await;
    assert_eq!(rt.get_by_key("intent").await, Some(id));
    assert_eq!(rt.get_by_key("nonexistent").await, None);
}

// ── 2. update_belief returns System1Only ────────────────────────────────────────

#[tokio::test]
async fn update_belief_returns_system1_only_without_client() {
    let rt = rt();
    // Confidence diverges (0.9 vs baseline 0.5 → divergence 0.4 > τ 0.15)
    // but no LLM client → System1Only
    let id = rt.insert_belief(asserted("k", "v", 0.9)).await;
    let result = rt.update_belief(
        id,
        BeliefValue::Asserted("v".into()),
        Provenance::UserStatement { turn: 1 },
        0.9,
    ).await.unwrap();
    assert!(matches!(result, RuntimeUpdateResult::System1Only));
}

// ── 3. checkpoint + rollback ───────────────────────────────────────────────────

#[tokio::test]
async fn checkpoint_and_rollback_round_trip() {
    let rt = rt();

    let id = rt.insert_belief(BeliefNode::new(
        "sensor",
        BeliefValue::Deterministic(json!(true)),
        Provenance::ToolResult { tool: "sensor".into(), call_id: uuid::Uuid::new_v4() },
        1.0,
    )).await;

    let cp = rt.checkpoint().await;

    // Mutate to a conflicting value so rollback has contradictions to process.
    rt.update_belief(
        id,
        BeliefValue::Deterministic(json!(false)),
        Provenance::ToolResult { tool: "sensor".into(), call_id: uuid::Uuid::new_v4() },
        1.0,
    ).await.unwrap();

    // Rollback should succeed and return a diff.
    let diff = rt.rollback_to(cp).await.expect("rollback should succeed");
    assert!(!diff.modified.is_empty() || !diff.added.is_empty() || !diff.removed.is_empty());
}

// ── 4. retrieve sorted by score ───────────────────────────────────────────────

#[tokio::test]
async fn retrieve_sorted_by_score_descending() {
    let rt = rt();
    // Insert beliefs with different (low) confidence so uncertainty_bonus varies.
    for i in 0..5u32 {
        let conf = 0.1 + i as f32 * 0.15; // 0.1, 0.25, 0.4, 0.55, 0.7
        rt.insert_belief(asserted(&format!("k{i}"), "v", conf)).await;
    }
    let results = rt.retrieve_for_query("anything", 10_000).await;
    assert!(!results.is_empty());
    // Scores must be non-increasing.
    for w in results.windows(2) {
        let s0 = epica_runtime::compute_score(0.0, w[0].fast_confidence, 0.0, 1.0);
        let s1 = epica_runtime::compute_score(0.0, w[1].fast_confidence, 0.0, 1.0);
        assert!(
            s0 >= s1 - 1e-5,
            "retrieve not sorted: {s0} < {s1} for beliefs {} vs {}",
            w[0].key,
            w[1].key,
        );
    }
}

// ── 5. expired beliefs are filtered ───────────────────────────────────────────

#[tokio::test]
async fn expired_beliefs_not_returned_by_retrieve() {
    let rt = rt();

    // Active belief
    rt.insert_belief(asserted("active", "v", 0.9)).await;

    // Already-expired belief (TTL = 1 ms, will expire immediately)
    let expired = BeliefNode::new(
        "expired",
        BeliefValue::Asserted("gone".into()),
        Provenance::UserStatement { turn: 0 },
        0.9,
    ).with_ttl_ms(1);
    rt.insert_belief(expired).await;

    // Sleep long enough for the TTL to lapse.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let results = rt.retrieve_for_query("", 10_000).await;
    assert!(
        results.iter().all(|b| b.key != "expired"),
        "expired belief should not appear in retrieval"
    );
    assert!(
        results.iter().any(|b| b.key == "active"),
        "active belief should appear in retrieval"
    );
}
