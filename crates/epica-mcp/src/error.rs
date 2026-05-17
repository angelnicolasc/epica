use axum::{http::StatusCode, response::IntoResponse, Json};
use serde_json::json;

/// Errors from MCP server operations.
#[derive(Debug, thiserror::Error)]
pub enum McpError {
    #[error("belief not found: {key}")]
    NotFound { key: String },

    #[error("runtime error: {0}")]
    Runtime(String),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("task not found: {task_id}")]
    TaskNotFound { task_id: uuid::Uuid },

    #[error("bad request: {0}")]
    BadRequest(String),
}

impl IntoResponse for McpError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match &self {
            McpError::NotFound { .. } => (StatusCode::NOT_FOUND, self.to_string()),
            McpError::TaskNotFound { .. } => (StatusCode::NOT_FOUND, self.to_string()),
            McpError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
        };
        (status, Json(json!({ "error": message }))).into_response()
    }
}
