//! SSE streaming for SEP-1686 Tasks primitive.
//!
//! `GET /v1/tasks/:id/stream` — Server-Sent Events endpoint.
//! The client initiates a task via `POST /v1/beliefs` (which may trigger System 2) and
//! receives a `task_id`. It then streams this endpoint until the task reaches a
//! terminal state (Completed or Failed) or a 30-second timeout.
//!
//! This completes the SEP-1686 call-now/fetch-later contract: callers can either
//! poll `GET /v1/tasks/:id` or subscribe to the stream for push notification.

use std::{convert::Infallible, sync::Arc, time::Duration};

use axum::{
    extract::{Path, State},
    response::{
        sse::{Event, KeepAlive, Sse},
    },
};
use futures_util::stream::{self, Stream};
use uuid::Uuid;

use crate::{tasks::TaskStatus, AppState};

/// `GET /v1/tasks/:id/stream` — stream task status until terminal or timeout (30 s).
pub async fn handle_task_stream(
    State(state): State<Arc<AppState>>,
    Path(task_id): Path<Uuid>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    // 300 ticks × 100 ms = 30 s timeout
    let stream = stream::unfold(
        (state, task_id, 0u32, false),
        |(state, id, tick, done)| async move {
            if done || tick > 300 {
                return None;
            }

            tokio::time::sleep(Duration::from_millis(100)).await;

            let status = state.task_store.get(id).ok().flatten().map(|t| t.status);

            let (event, next_done) = match status {
                None => {
                    let ev = Event::default()
                        .event("error")
                        .data(r#"{"error":"task not found"}"#);
                    (ev, true)
                }
                Some(s) => {
                    let is_terminal =
                        matches!(s, TaskStatus::Completed { .. } | TaskStatus::Failed { .. });
                    let json = serde_json::to_string(&s).unwrap_or_else(|_| "{}".into());
                    let ev = Event::default().event("status").data(json);
                    (ev, is_terminal)
                }
            };

            Some((Ok(event), (state, id, tick + 1, next_done)))
        },
    );

    Sse::new(stream).keep_alive(KeepAlive::default())
}
