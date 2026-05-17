//! MCP 2026 Server Card for `.well-known/epica-server-card.json`.
//!
//! Server Cards are MCP 2026's machine-readable API discovery format — analogous to
//! OpenAPI but oriented around agent-to-agent and tool-to-tool negotiation. They are
//! served without authentication from `/.well-known/` so clients can discover
//! capabilities without prior knowledge of the server's API surface.
//!
//! The belief_schema and contract_schema fields use JSON Schema Draft 2020-12.

use serde::{Deserialize, Serialize};
use serde_json::json;

/// MCP 2026 Server Card — machine-readable capability discovery.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerCard {
    /// Human-readable server name.
    pub name: String,
    pub version: String,
    pub description: String,
    /// MCP protocol version this server implements.
    pub mcp_version: String,
    /// All available endpoints with full JSON Schema for inputs/outputs.
    pub endpoints: Vec<EndpointDescriptor>,
    /// JSON Schema for a `BeliefNode` as seen through the REST API.
    pub belief_schema: serde_json::Value,
    /// JSON Schema for contract status response.
    pub contract_schema: serde_json::Value,
    /// OAuth 2.1 metadata per MCP 2026 enterprise auth spec.
    pub oauth: OAuthMetadata,
    /// Epica-specific capabilities declared for agent negotiation.
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointDescriptor {
    pub name: String,
    pub method: String,
    pub path: String,
    pub description: String,
    pub input_schema: serde_json::Value,
    pub output_schema: serde_json::Value,
    /// True if the endpoint may return a `task_id` (SEP-1686 Tasks primitive).
    pub is_async: bool,
}

/// OAuth 2.1 metadata block embedded in the Server Card.
///
/// Clients read this to discover how to obtain a Bearer token before calling
/// authenticated endpoints. `/.well-known/jwks.json` lists the public key(s)
/// used for token validation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthMetadata {
    /// Token endpoint where clients exchange credentials for access tokens.
    pub token_endpoint: String,
    /// JWKS endpoint for public key discovery (RS256 in production).
    pub jwks_uri: String,
    /// Supported grant types per OAuth 2.1.
    pub grant_types_supported: Vec<String>,
    /// Scopes recognized by this server.
    pub scopes_supported: Vec<String>,
}

impl McpServerCard {
    pub fn default_card() -> Self {
        let base = "http://localhost:8080";

        Self {
            name: "epica-mcp".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            description: concat!(
                "Epica BeliefQuad MCP 2026 server — formal AGM belief revision (K*2–K*6), ",
                "dual-process uncertainty containment (System 1 + System 2), ",
                "behavioral contracts C=(P,I,G,R), and all nine Mnemonic Sovereignty primitives. ",
                "First production Rust implementation without an external graph database."
            ).into(),
            mcp_version: "2026-03".into(),
            endpoints: vec![
                EndpointDescriptor {
                    name: "belief.get".into(),
                    method: "GET".into(),
                    path: "/v1/beliefs/:key".into(),
                    description: "Read a belief by key with dual-process confidence scores.".into(),
                    input_schema: json!({
                        "$schema": "https://json-schema.org/draft/2020-12/schema",
                        "type": "object",
                        "required": ["key"],
                        "properties": {
                            "key": { "type": "string", "description": "Belief identifier" }
                        }
                    }),
                    output_schema: belief_get_schema(),
                    is_async: false,
                },
                EndpointDescriptor {
                    name: "belief.set".into(),
                    method: "POST".into(),
                    path: "/v1/beliefs".into(),
                    description: concat!(
                        "Insert or AGM-revise a belief. System 1 always runs. ",
                        "If confidence divergence exceeds τ, System 2 activates and a ",
                        "SEP-1686 task_id is returned for async result retrieval."
                    ).into(),
                    input_schema: belief_set_input_schema(),
                    output_schema: belief_set_output_schema(),
                    is_async: true,
                },
                EndpointDescriptor {
                    name: "task.get".into(),
                    method: "GET".into(),
                    path: "/v1/tasks/:id".into(),
                    description: "Poll a SEP-1686 task status (System 2 result).".into(),
                    input_schema: json!({
                        "type": "object",
                        "required": ["id"],
                        "properties": { "id": { "type": "string", "format": "uuid" } }
                    }),
                    output_schema: task_output_schema(),
                    is_async: false,
                },
                EndpointDescriptor {
                    name: "task.stream".into(),
                    method: "GET".into(),
                    path: "/v1/tasks/:id/stream".into(),
                    description: "SSE stream of task status — push alternative to polling.".into(),
                    input_schema: json!({
                        "type": "object",
                        "required": ["id"],
                        "properties": { "id": { "type": "string", "format": "uuid" } }
                    }),
                    output_schema: json!({
                        "description": "Server-Sent Events stream. Events: 'status' | 'error'. Data: TaskStatus JSON."
                    }),
                    is_async: true,
                },
                EndpointDescriptor {
                    name: "checkpoint".into(),
                    method: "POST".into(),
                    path: "/v1/checkpoint".into(),
                    description: "Save an immutable snapshot of the current BeliefQuad state.".into(),
                    input_schema: json!({ "type": "object", "properties": {} }),
                    output_schema: json!({
                        "type": "object",
                        "required": ["checkpoint_id", "version"],
                        "properties": {
                            "checkpoint_id": { "type": "string" },
                            "version": { "type": "integer", "minimum": 0 }
                        }
                    }),
                    is_async: false,
                },
                EndpointDescriptor {
                    name: "rollback".into(),
                    method: "POST".into(),
                    path: "/v1/rollback".into(),
                    description: concat!(
                        "AGM-verified rollback to a checkpoint. ",
                        "Verifies K*4 (vacuity) — refuses unnecessary contractions. ",
                        "Returns BeliefQuadDiff as a structured root-cause report."
                    ).into(),
                    input_schema: json!({
                        "type": "object",
                        "required": ["checkpoint_id"],
                        "properties": { "checkpoint_id": { "type": "string" } }
                    }),
                    output_schema: diff_schema(),
                    is_async: false,
                },
                EndpointDescriptor {
                    name: "query".into(),
                    method: "POST".into(),
                    path: "/v1/query".into(),
                    description: concat!(
                        "Multicriteria belief retrieval — combines prospective index similarity, ",
                        "uncertainty bonus, causal centrality, and temporal decay."
                    ).into(),
                    input_schema: json!({
                        "type": "object",
                        "required": ["query"],
                        "properties": {
                            "query": { "type": "string" },
                            "budget_tokens": {
                                "type": "integer",
                                "minimum": 1,
                                "default": 4096,
                                "description": "Token budget for result packing."
                            }
                        }
                    }),
                    output_schema: json!({
                        "type": "object",
                        "properties": {
                            "beliefs": {
                                "type": "array",
                                "items": { "$ref": "#/belief_schema" }
                            }
                        }
                    }),
                    is_async: false,
                },
                EndpointDescriptor {
                    name: "counterfactual".into(),
                    method: "POST".into(),
                    path: "/v1/counterfactual".into(),
                    description: "Pure CausalGraph traversal: surviving beliefs if antecedent belief had never existed.".into(),
                    input_schema: json!({
                        "type": "object",
                        "required": ["belief_key"],
                        "properties": { "belief_key": { "type": "string" } }
                    }),
                    output_schema: json!({
                        "type": "object",
                        "required": ["surviving", "excluded_count"],
                        "properties": {
                            "surviving": { "type": "array", "items": { "type": "object" } },
                            "excluded_count": { "type": "integer", "minimum": 0 }
                        }
                    }),
                    is_async: false,
                },
                EndpointDescriptor {
                    name: "diff".into(),
                    method: "POST".into(),
                    path: "/v1/diff".into(),
                    description: "Structural diff between current BeliefQuad and a checkpoint, including Trajectory-ECE.".into(),
                    input_schema: json!({
                        "type": "object",
                        "required": ["checkpoint_id"],
                        "properties": { "checkpoint_id": { "type": "string" } }
                    }),
                    output_schema: diff_schema(),
                    is_async: false,
                },
                EndpointDescriptor {
                    name: "contract.status".into(),
                    method: "GET".into(),
                    path: "/v1/contract/status".into(),
                    description: "Real-time behavioral contract drift bounds D* = α/γ.".into(),
                    input_schema: json!({ "type": "object", "properties": {} }),
                    output_schema: contract_status_schema(),
                    is_async: false,
                },
            ],
            belief_schema: belief_node_schema(),
            contract_schema: contract_status_schema(),
            oauth: OAuthMetadata {
                token_endpoint: format!("{base}/oauth/token"),
                jwks_uri: format!("{base}/.well-known/jwks.json"),
                grant_types_supported: vec!["client_credentials".into(), "authorization_code".into()],
                scopes_supported: vec![
                    "beliefs:read".into(),
                    "beliefs:write".into(),
                    "contracts:read".into(),
                    "checkpoints:write".into(),
                ],
            },
            capabilities: vec![
                "agm-belief-revision".into(),
                "dual-process-uncertainty".into(),
                "behavioral-contracts".into(),
                "mnemonic-sovereignty".into(),
                "prospective-indexing".into(),
                "sep-1686-tasks".into(),
                "causal-counterfactuals".into(),
            ],
        }
    }
}

fn belief_node_schema() -> serde_json::Value {
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "epica:BeliefNode",
        "type": "object",
        "required": ["key", "value", "fast_confidence", "provenance"],
        "properties": {
            "key": {
                "type": "string",
                "description": "Unique belief identifier within the session."
            },
            "value": {
                "description": "Belief content: deterministic tool result, LLM inference, or user assertion."
            },
            "fast_confidence": {
                "type": "number",
                "minimum": 0.0,
                "maximum": 1.0,
                "description": "System 1 confidence — Noisy-OR propagation over the CausalGraph."
            },
            "slow_confidence": {
                "type": ["number", "null"],
                "minimum": 0.0,
                "maximum": 1.0,
                "description": "System 2 confidence — present only when LLM reflection was activated (divergence > τ)."
            },
            "provenance": {
                "type": "string",
                "description": "Epistemic source: ToolResult | LlmInference | UserStatement | RuntimeInference"
            }
        }
    })
}

fn belief_get_schema() -> serde_json::Value {
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "required": ["key", "value", "fast_confidence", "provenance"],
        "properties": {
            "key": { "type": "string" },
            "value": {},
            "fast_confidence": { "type": "number", "minimum": 0.0, "maximum": 1.0 },
            "slow_confidence": { "type": ["number", "null"] },
            "provenance": { "type": "string" }
        }
    })
}

fn belief_set_input_schema() -> serde_json::Value {
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "required": ["key", "value", "confidence"],
        "properties": {
            "key": { "type": "string" },
            "value": { "description": "Any JSON value — stored as BeliefValue::Asserted." },
            "confidence": {
                "type": "number",
                "minimum": 0.0,
                "maximum": 1.0,
                "description": "Initial fast_confidence for this belief."
            },
            "provenance_kind": {
                "type": "string",
                "enum": ["user", "tool", "llm:<model>"],
                "default": "user"
            }
        }
    })
}

fn belief_set_output_schema() -> serde_json::Value {
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "required": ["belief_id", "system2_triggered"],
        "properties": {
            "belief_id": { "type": "string" },
            "task_id": {
                "type": ["string", "null"],
                "format": "uuid",
                "description": "SEP-1686 task ID — present when System 2 was activated."
            },
            "system2_triggered": {
                "type": "boolean",
                "description": "True when System 2 ran and a task_id was issued."
            }
        }
    })
}

fn task_output_schema() -> serde_json::Value {
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "required": ["task_id", "belief_key", "status"],
        "properties": {
            "task_id": { "type": "string", "format": "uuid" },
            "belief_key": { "type": "string" },
            "created_at_ms": { "type": "integer" },
            "status": {
                "type": "object",
                "required": ["type"],
                "properties": {
                    "type": { "type": "string", "enum": ["pending", "running", "completed", "failed"] },
                    "result": { "description": "Present when type=completed." },
                    "error": { "type": "string", "description": "Present when type=failed." }
                }
            }
        }
    })
}

fn diff_schema() -> serde_json::Value {
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "required": ["added", "removed", "modified", "trajectory_ece"],
        "properties": {
            "added": { "type": "integer", "minimum": 0 },
            "removed": { "type": "integer", "minimum": 0 },
            "modified": { "type": "integer", "minimum": 0 },
            "trajectory_ece": {
                "type": "number",
                "description": "Trajectory-ECE over the diff interval. Target < 0.08 per AUQ paper (arXiv:2601.15703)."
            }
        }
    })
}

fn contract_status_schema() -> serde_json::Value {
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "oneOf": [
            {
                "type": "object",
                "required": ["expected_drift"],
                "properties": {
                    "alpha": {
                        "type": "number",
                        "description": "Natural drift rate α of the agent."
                    },
                    "gamma": {
                        "type": "number",
                        "description": "Contract enforcement rate γ."
                    },
                    "expected_drift": {
                        "type": "number",
                        "description": "D* = α/γ — expected drift under the contract."
                    },
                    "gaussian_concentration": {
                        "type": "number",
                        "description": "CLT concentration bound √(D*(1-D*))."
                    },
                    "satisfies_delta_1_k_100": {
                        "type": "boolean",
                        "description": "True if D* satisfies δ=1 violation in k=100 steps."
                    }
                }
            },
            {
                "type": "object",
                "required": ["status"],
                "properties": {
                    "status": { "type": "string", "const": "no contract configured" }
                }
            }
        ]
    })
}
