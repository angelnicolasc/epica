//! Integration test: SledTaskStore survives a simulated process restart.
//!
//! The contract under test: a task inserted into a sled-backed store with a
//! pending status MUST still be readable, with its original payload, after
//! the store handle is dropped and reopened from the same path.
//!
//! This is the lifeline the in-memory backend cannot provide and is the entire
//! motivation for TD-P5-002. The test runs only when the `sled-store` feature
//! is enabled.

#![cfg(feature = "sled-store")]

use epica_mcp::tasks::{McpTask, SledTaskStore, TaskStatus, TaskStore};
use uuid::Uuid;

/// Build a fresh sled path under the OS temp dir, scoped to the running process
/// and a random nonce so concurrent test runs do not collide.
fn tmp_sled_path() -> std::path::PathBuf {
    let base = std::env::temp_dir();
    let nonce = Uuid::new_v4();
    base.join(format!("epica-test-sled-{nonce}"))
}

#[test]
fn sled_task_store_survives_restart() {
    let path = tmp_sled_path();
    let task_id = Uuid::new_v4();

    // ── Phase 1: open the store, insert a Pending task, drop the handle.
    {
        let store = SledTaskStore::open(&path).expect("open sled store (phase 1)");
        store
            .insert(McpTask {
                task_id,
                belief_key: "auth_intent".to_string(),
                status: TaskStatus::Pending,
                created_at_ms: 1_700_000_000_000,
            })
            .expect("insert pending task");
        // store drops here — sled flushes on Drop.
    }

    // ── Phase 2: simulate a process restart by reopening the same path.
    {
        let store = SledTaskStore::open(&path).expect("open sled store (phase 2)");
        let recovered = store
            .get(task_id)
            .expect("get on reopened store")
            .expect("task must survive restart");

        assert_eq!(recovered.task_id, task_id);
        assert_eq!(recovered.belief_key, "auth_intent");
        assert_eq!(recovered.created_at_ms, 1_700_000_000_000);
        assert!(
            matches!(recovered.status, TaskStatus::Pending),
            "status must remain Pending after restart, got {:?}",
            recovered.status
        );
    }

    // ── Phase 3: transition to Completed and verify the new status persists too.
    {
        let store = SledTaskStore::open(&path).expect("open sled store (phase 3)");
        store
            .update_status(
                task_id,
                TaskStatus::Completed {
                    result: serde_json::json!({"revised_confidence": 0.87}),
                },
            )
            .expect("update status");
    }
    {
        let store = SledTaskStore::open(&path).expect("open sled store (phase 4)");
        let recovered = store
            .get(task_id)
            .expect("get on reopened store")
            .expect("task still present");

        match recovered.status {
            TaskStatus::Completed { result } => {
                let conf = result.get("revised_confidence").and_then(|v| v.as_f64());
                assert_eq!(conf, Some(0.87), "completed payload must round-trip exactly");
            }
            other => panic!("expected Completed after restart, got {other:?}"),
        }
    }

    // Best-effort cleanup; ignore failures because Windows may hold the dir briefly.
    let _ = std::fs::remove_dir_all(&path);
}
