//! Write-time prospective indexing prompt + OpenAI tool schema.
//!
//! Mirrors `epica-anthropic::prompt::build_prospective_message` so that the
//! observable behaviour of the two providers is comparable: a swap from
//! Anthropic to OpenAI should change the model and the cost, never the
//! semantics of the scenarios stored in the `ProspectiveIndex`.

use serde_json::{json, Value};

/// Build the user message for a write-time prospective scenario generation call.
///
/// Identical wording to the Anthropic counterpart on purpose: cross-provider
/// retrieval quality comparisons reflect model differences, not prompt drift.
pub fn build_prospective_message(belief_key: &str, belief_value_repr: &str) -> String {
    format!(
        "You are the write-time prospective indexer of an epistemic belief runtime \
         (Epica, grounded in Kumiho arXiv:2603.17244).\n\n\
         A new belief has been inserted into the BeliefQuad:\n\
         Key  : {key}\n\
         Value: {value}\n\n\
         Your task: generate 3 to 5 concrete, specific future queries that an \
         agent might ask WHERE THIS BELIEF WOULD BE THE DECISIVE PIECE OF CONTEXT.\n\n\
         Requirements:\n\
         - Each query must be a natural-language question (not a keyword list).\n\
         - Queries must be retrievable from this belief — do not invent new facts.\n\
         - Vary the query intent: some about actions, some about reasoning, some \
           about dependencies.\n\n\
         Call `generate_scenarios` with your list.",
        key   = belief_key,
        value = belief_value_repr,
    )
}

/// OpenAI `tools[]` schema for the `generate_scenarios` function.
///
/// Forced via `tool_choice: { type: "function", function: { name: "generate_scenarios" } }`,
/// guaranteeing structured JSON output — no free-form text to parse.
pub fn generate_scenarios_function_schema() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": "generate_scenarios",
            "description": "Return prospective query scenarios for the inserted belief.",
            "parameters": {
                "type": "object",
                "properties": {
                    "scenarios": {
                        "type": "array",
                        "minItems": 1,
                        "maxItems": 5,
                        "items": {
                            "type": "object",
                            "properties": {
                                "query": {
                                    "type": "string",
                                    "description": "A future natural-language query this belief helps answer."
                                }
                            },
                            "required": ["query"]
                        }
                    }
                },
                "required": ["scenarios"]
            }
        }
    })
}
