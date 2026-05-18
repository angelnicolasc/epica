//! MCP Tasks primitive (SEP-1686): call-now / fetch-later for System 2 operations.
//!
//! When a belief update triggers System 2 (LLM reflection), the runtime returns
//! `RuntimeUpdateResult::System2Pending`. The MCP handler creates a `McpTask`,
//! stores it via the [`TaskStore`] trait, and returns the `task_id` to the caller.
//! The caller can then:
//!   - Poll: `GET /v1/tasks/:id`
//!   - Stream: `GET /v1/tasks/:id/stream` (Server-Sent Events)
//!
//! ## Backends
//!
//! - [`InMemoryTaskStore`] (default): `HashMap<Uuid, McpTask>`. Fastest; loses
//!   in-flight tasks across server restart. Suitable for single-process
//!   deployments where the restart blast radius is acceptable.
//! - [`SledTaskStore`] (feature `sled-store`): embedded KV store with crash
//!   recovery. In-flight tasks survive process exit. Suitable for production
//!   single-node deployments. Selected at runtime via
//!   `EPICA_TASKSTORE=sled:/path/to/dir`.
//!
//! TD-P5-002 (sled backend) — closed by this commit.

use std::sync::Mutex;
use uuid::Uuid;

/// Status of an async MCP task (SEP-1686).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    Running,
    Completed { result: serde_json::Value },
    Failed { error: String },
}

/// A pending or completed async task.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct McpTask {
    pub task_id: Uuid,
    pub belief_key: String,
    pub status: TaskStatus,
    pub created_at_ms: u64,
}

/// Errors a [`TaskStore`] implementation can surface.
#[derive(Debug, thiserror::Error)]
pub enum TaskStoreError {
    #[error("task store backend failure: {0}")]
    Backend(String),

    #[error("serialization failure: {0}")]
    Serde(#[from] serde_json::Error),
}

/// Persistence-agnostic interface for the SEP-1686 task store.
///
/// Implementations must be thread-safe (`Send + Sync`). All methods are
/// synchronous because the in-memory backend has no need to await and the
/// sled backend's I/O is fast enough to keep on the request thread for the
/// task-store working set sizes Epica targets (≤ 10⁴ live tasks per node).
pub trait TaskStore: Send + Sync {
    /// Fetch a task by ID. Returns `Ok(None)` when the task does not exist.
    fn get(&self, task_id: Uuid) -> Result<Option<McpTask>, TaskStoreError>;

    /// Insert a new task, overwriting any prior entry with the same ID.
    fn insert(&self, task: McpTask) -> Result<(), TaskStoreError>;

    /// Update the status of an existing task. No-op when the task is absent.
    fn update_status(&self, task_id: Uuid, status: TaskStatus) -> Result<(), TaskStoreError>;
}

// ── In-memory backend ────────────────────────────────────────────────────────

/// In-memory backend: `HashMap<Uuid, McpTask>` behind a `Mutex`.
#[derive(Default)]
pub struct InMemoryTaskStore {
    tasks: Mutex<std::collections::HashMap<Uuid, McpTask>>,
}

impl InMemoryTaskStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl TaskStore for InMemoryTaskStore {
    fn get(&self, task_id: Uuid) -> Result<Option<McpTask>, TaskStoreError> {
        Ok(self.tasks.lock().unwrap().get(&task_id).cloned())
    }

    fn insert(&self, task: McpTask) -> Result<(), TaskStoreError> {
        self.tasks.lock().unwrap().insert(task.task_id, task);
        Ok(())
    }

    fn update_status(&self, task_id: Uuid, status: TaskStatus) -> Result<(), TaskStoreError> {
        if let Some(task) = self.tasks.lock().unwrap().get_mut(&task_id) {
            task.status = status;
        }
        Ok(())
    }
}

// ── Sled backend ─────────────────────────────────────────────────────────────

#[cfg(feature = "sled-store")]
mod sled_backend {
    use super::{McpTask, TaskStatus, TaskStore, TaskStoreError};
    use uuid::Uuid;

    /// Embedded sled-backed task store. Survives process restart.
    ///
    /// Keys are the canonical 16-byte UUID; values are `serde_json` blobs of
    /// the full [`McpTask`]. Writes flush implicitly via sled's WAL; explicit
    /// `flush()` is not required for correctness, only durability latency.
    pub struct SledTaskStore {
        db: sled::Db,
    }

    impl SledTaskStore {
        /// Open or create a sled database at `path`.
        pub fn open(path: impl AsRef<std::path::Path>) -> Result<Self, TaskStoreError> {
            let db = sled::open(path).map_err(|e| TaskStoreError::Backend(e.to_string()))?;
            Ok(Self { db })
        }

        fn key(task_id: Uuid) -> [u8; 16] {
            *task_id.as_bytes()
        }
    }

    impl TaskStore for SledTaskStore {
        fn get(&self, task_id: Uuid) -> Result<Option<McpTask>, TaskStoreError> {
            let key = Self::key(task_id);
            match self.db.get(key).map_err(|e| TaskStoreError::Backend(e.to_string()))? {
                None => Ok(None),
                Some(ivec) => {
                    let task = serde_json::from_slice::<McpTask>(&ivec)?;
                    Ok(Some(task))
                }
            }
        }

        fn insert(&self, task: McpTask) -> Result<(), TaskStoreError> {
            let key = Self::key(task.task_id);
            let bytes = serde_json::to_vec(&task)?;
            self.db.insert(key, bytes).map_err(|e| TaskStoreError::Backend(e.to_string()))?;
            Ok(())
        }

        fn update_status(&self, task_id: Uuid, status: TaskStatus) -> Result<(), TaskStoreError> {
            let key = Self::key(task_id);
            let Some(existing) = self.db.get(key).map_err(|e| TaskStoreError::Backend(e.to_string()))? else {
                return Ok(());
            };
            let mut task = serde_json::from_slice::<McpTask>(&existing)?;
            task.status = status;
            let bytes = serde_json::to_vec(&task)?;
            self.db.insert(key, bytes).map_err(|e| TaskStoreError::Backend(e.to_string()))?;
            Ok(())
        }
    }
}

#[cfg(feature = "sled-store")]
pub use sled_backend::SledTaskStore;

// ── Backend selection from environment ───────────────────────────────────────

/// Construct a [`TaskStore`] backend from the `EPICA_TASKSTORE` env var.
///
/// Supported values:
/// - unset, `"memory"`, `"in-memory"` → [`InMemoryTaskStore`]
/// - `"sled:/path/to/dir"` (requires `sled-store` feature) → [`SledTaskStore`]
///
/// Falls back to in-memory on any parsing failure with a `tracing::warn!`.
pub fn task_store_from_env() -> Box<dyn TaskStore> {
    let spec = std::env::var("EPICA_TASKSTORE").unwrap_or_default();
    let trimmed = spec.trim();

    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("memory") || trimmed.eq_ignore_ascii_case("in-memory") {
        return Box::new(InMemoryTaskStore::new());
    }

    #[cfg(feature = "sled-store")]
    if let Some(path) = trimmed.strip_prefix("sled:") {
        match SledTaskStore::open(path) {
            Ok(store) => {
                tracing::info!("task store: sled-backed at {}", path);
                return Box::new(store);
            }
            Err(e) => {
                tracing::warn!("failed to open sled task store at {}: {} — falling back to in-memory", path, e);
            }
        }
    }

    #[cfg(not(feature = "sled-store"))]
    if trimmed.starts_with("sled:") {
        tracing::warn!("EPICA_TASKSTORE=sled:* requested but binary was built without --features sled-store; falling back to in-memory");
    }

    tracing::info!("task store: in-memory (default)");
    Box::new(InMemoryTaskStore::new())
}
