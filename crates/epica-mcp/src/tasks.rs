//! MCP Tasks primitive (SEP-1686): call-now / fetch-later for System 2 operations.
//!
//! When a belief update triggers System 2 (LLM reflection), the runtime returns
//! `RuntimeUpdateResult::System2Activated`. The MCP handler creates a `McpTask`,
//! stores it here, and returns the `task_id` to the caller. The caller can then:
//!   - Poll: `GET /v1/tasks/:id`
//!   - Stream: `GET /v1/tasks/:id/stream` (Server-Sent Events)
//!
//! TD-P5-002: Replace the in-memory HashMap with a durable store (sled, Redis) for
//! multi-process deployments where the HTTP server may restart between dispatch and poll.

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
#[derive(Debug, Clone, serde::Serialize)]
pub struct McpTask {
    pub task_id: Uuid,
    pub belief_key: String,
    pub status: TaskStatus,
    pub created_at_ms: u64,
}

/// In-memory task store.
#[derive(Default)]
pub struct TaskStore {
    tasks: std::collections::HashMap<Uuid, McpTask>,
}

impl TaskStore {
    pub fn get(&self, task_id: Uuid) -> Option<&McpTask> {
        self.tasks.get(&task_id)
    }

    pub fn insert(&mut self, task: McpTask) {
        self.tasks.insert(task.task_id, task);
    }

    pub fn update_status(&mut self, task_id: Uuid, status: TaskStatus) {
        if let Some(task) = self.tasks.get_mut(&task_id) {
            task.status = status;
        }
    }
}
