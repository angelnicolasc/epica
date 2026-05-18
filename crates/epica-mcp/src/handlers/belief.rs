//! Handlers for belief CRUD endpoints.

use axum::extract::{Path, State};
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use epica_core::{BeliefNode, BeliefValue, Provenance};

use crate::{
    error::McpError,
    tasks::{McpTask, TaskStatus},
    telemetry::names,
    AppState,
};

#[derive(Debug, Serialize)]
pub struct BeliefGetResponse {
    pub key: String,
    pub value: serde_json::Value,
    pub fast_confidence: f32,
    pub slow_confidence: Option<f32>,
    pub provenance: String,
}

#[derive(Debug, Deserialize)]
pub struct BeliefSetRequest {
    pub key: String,
    pub value: serde_json::Value,
    pub confidence: f32,
    /// `"user"` (default) | `"llm:<model>"` | `"tool:<name>"`
    pub provenance_kind: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct BeliefSetResponse {
    pub belief_id: String,
    /// SEP-1686 task ID — present when System 2 was activated.
    /// Poll `GET /v1/tasks/:id` or stream `GET /v1/tasks/:id/stream`.
    pub task_id: Option<Uuid>,
    pub system2_triggered: bool,
}

/// `GET /v1/beliefs/:key`
pub async fn handle_belief_get(
    State(state): State<Arc<AppState>>,
    Path(key): Path<String>,
) -> Result<Json<BeliefGetResponse>, McpError> {
    let quad = state.runtime.read_quad().await;
    let (_, node) = quad
        .iter()
        .find(|(_, n)| n.key == key)
        .ok_or_else(|| McpError::NotFound { key: key.clone() })?;

    let value_json = match &node.value {
        BeliefValue::Deterministic(v) | BeliefValue::Inferred(v) => v.clone(),
        BeliefValue::Asserted(s) => serde_json::Value::String(s.clone()),
        BeliefValue::Reference(id) => serde_json::json!({ "reference": format!("{id:?}") }),
    };

    Ok(Json(BeliefGetResponse {
        key: node.key.clone(),
        value: value_json,
        fast_confidence: node.fast_confidence,
        slow_confidence: node.slow_confidence,
        provenance: format!("{:?}", node.provenance),
    }))
}

/// `POST /v1/beliefs`
///
/// Inserts a new belief or revises an existing one via AGM belief revision.
/// System 1 always runs. If System 2 activates (confidence divergence > τ),
/// a SEP-1686 Task is created and its ID is returned in the response.
pub async fn handle_belief_set(
    State(state): State<Arc<AppState>>,
    Json(req): Json<BeliefSetRequest>,
) -> Result<Json<BeliefSetResponse>, McpError> {
    let provenance = parse_provenance(req.provenance_kind.as_deref());

    // Check if belief exists → insert or update
    let existing_id = state.runtime.get_by_key(&req.key).await;

    if let Some(id) = existing_id {
        let value = BeliefValue::Asserted(req.value.to_string());
        let result = state
            .runtime
            .update_belief(id, value, provenance, req.confidence)
            .await
            .map_err(|e| McpError::Runtime(e.to_string()))?;

        match result {
            // Legacy sync path (used in direct / test scenarios without the server).
            epica_runtime::RuntimeUpdateResult::System2Activated { result: sys2, .. } => {
                metrics::counter!(names::BELIEF_UPDATES, "system2" => "true").increment(1);
                metrics::counter!(names::SYSTEM2_ACTIVATIONS).increment(1);

                let task_id = Uuid::new_v4();
                let now_ms = now_ms();
                let _ = state.task_store.insert(McpTask {
                    task_id,
                    belief_key: req.key.clone(),
                    status: TaskStatus::Completed {
                        result: serde_json::json!({
                            "revised_confidence": sys2.revised_confidence,
                        }),
                    },
                    created_at_ms: now_ms,
                });

                Ok(Json(BeliefSetResponse {
                    belief_id: format!("{id:?}"),
                    task_id: Some(task_id),
                    system2_triggered: true,
                }))
            }

            // Async path: budget consumed here, LLM called in spawned task.
            epica_runtime::RuntimeUpdateResult::System2Pending { signal } => {
                // Try to consume budget synchronously before spawning.
                let has_budget = state.runtime.try_consume_system2_budget().await;
                if !has_budget {
                    metrics::counter!(names::BELIEF_UPDATES, "system2" => "throttled").increment(1);
                    metrics::counter!(names::SYSTEM2_THROTTLED).increment(1);
                    return Ok(Json(BeliefSetResponse {
                        belief_id: format!("{id:?}"),
                        task_id: None,
                        system2_triggered: false,
                    }));
                }

                let task_id = Uuid::new_v4();
                let _ = state.task_store.insert(McpTask {
                    task_id,
                    belief_key: req.key.clone(),
                    status: TaskStatus::Pending,
                    created_at_ms: now_ms(),
                });

                metrics::counter!(names::BELIEF_UPDATES, "system2" => "true").increment(1);

                let state_clone = Arc::clone(&state);
                tokio::spawn(async move {
                    let _ = state_clone.task_store
                        .update_status(task_id, TaskStatus::Running);

                    if let Some(client) = state_clone.runtime.llm_client_arc() {
                        match client.reflect(&signal).await {
                            Ok(result) => {
                                state_clone.runtime
                                    .apply_system2_result(id, result.revised_confidence)
                                    .await;
                                metrics::counter!(names::SYSTEM2_ACTIVATIONS).increment(1);
                                let _ = state_clone.task_store.update_status(
                                    task_id,
                                    TaskStatus::Completed {
                                        result: serde_json::json!({
                                            "revised_confidence": result.revised_confidence,
                                            "reasoning": result.reasoning,
                                        }),
                                    },
                                );
                            }
                            Err(e) => {
                                // Refund the budget token — transient errors must not drain
                                // the session budget.
                                state_clone.runtime.release_system2_budget().await;
                                let _ = state_clone.task_store.update_status(
                                    task_id,
                                    TaskStatus::Failed { error: e.to_string() },
                                );
                            }
                        }
                    }
                });

                Ok(Json(BeliefSetResponse {
                    belief_id: format!("{id:?}"),
                    task_id: Some(task_id),
                    system2_triggered: true,
                }))
            }

            epica_runtime::RuntimeUpdateResult::System2Throttled => {
                metrics::counter!(names::BELIEF_UPDATES, "system2" => "throttled").increment(1);
                metrics::counter!(names::SYSTEM2_THROTTLED).increment(1);
                Ok(Json(BeliefSetResponse {
                    belief_id: format!("{id:?}"),
                    task_id: None,
                    system2_triggered: false,
                }))
            }

            epica_runtime::RuntimeUpdateResult::System1Only => {
                metrics::counter!(names::BELIEF_UPDATES, "system2" => "false").increment(1);
                Ok(Json(BeliefSetResponse {
                    belief_id: format!("{id:?}"),
                    task_id: None,
                    system2_triggered: false,
                }))
            }
        }
    } else {
        let node = BeliefNode::new(
            &req.key,
            BeliefValue::Asserted(req.value.to_string()),
            provenance,
            req.confidence,
        );
        let belief_id = state.runtime.insert_belief(node).await;
        metrics::counter!(names::BELIEF_INSERTS).increment(1);

        Ok(Json(BeliefSetResponse {
            belief_id: format!("{belief_id:?}"),
            task_id: None,
            system2_triggered: false,
        }))
    }
}

/// `GET /v1/tasks/:id` — fetch SEP-1686 task status (poll variant).
pub async fn handle_belief_set_result(
    State(state): State<Arc<AppState>>,
    Path(task_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, McpError> {
    let task = state
        .task_store
        .get(task_id)
        .map_err(|e| McpError::Runtime(format!("task store error: {e}")))?
        .ok_or(McpError::TaskNotFound { task_id })?;
    Ok(Json(serde_json::json!({
        "task_id": task.task_id,
        "belief_key": task.belief_key,
        "status": task.status,
        "created_at_ms": task.created_at_ms,
    })))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn parse_provenance(kind: Option<&str>) -> Provenance {
    match kind.unwrap_or("user") {
        "tool" => Provenance::ToolResult {
            tool: "external".into(),
            call_id: Uuid::new_v4(),
        },
        s if s.starts_with("llm") => Provenance::LlmInference {
            model: s.trim_start_matches("llm:").to_string(),
            call_id: Uuid::new_v4(),
            prompt_hash: 0,
        },
        _ => Provenance::UserStatement { turn: 0 },
    }
}
