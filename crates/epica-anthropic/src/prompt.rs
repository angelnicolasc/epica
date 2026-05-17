//! System 2 UAR prompt construction and Anthropic tool schema.
//!
//! The prompt implements the Uncertainty-Aware Reflection (UAR) formulation
//! from Agentic UQ (arXiv:2601.15703): System 2 treats confidence calibration
//! as an Inverse Optimization Problem — given the diagnostic signal, infer
//! the confidence that maximises the likelihood of a satisfactory outcome.

use serde_json::{json, Value};

use epica_runtime::DiagnosticSignal;

/// Build the user message content for a System 2 reflection call.
pub fn build_system2_message(diagnostic: &DiagnosticSignal) -> String {
    format!(
        "You are the System 2 (Uncertainty-Aware Reflection) module of an \
         epistemic belief runtime (Epica, grounded in arXiv:2601.15703).\n\n\
         A belief has triggered reflection because its System 1 confidence \
         diverges significantly from the agent's reliability baseline.\n\n\
         Belief key         : {key}\n\
         System 1 confidence: {fast:.4}\n\
         Reliability baseline: {baseline:.4}\n\
         Divergence          : {div:.4}\n\n\
         Using inverse optimisation over the agent's epistemic state, determine \
         the best-calibrated confidence for this belief. Reason through:\n\
         1. Is the divergence indicative of underconfidence or overconfidence?\n\
         2. What confidence level would maximise the likelihood of a \
            satisfactory downstream outcome?\n\
         3. How should the belief's provenance type and divergence magnitude \
            influence recalibration?\n\n\
         Call `report_confidence` with your recalibrated value and a brief \
         chain-of-thought.",
        key      = diagnostic.belief_key,
        fast     = diagnostic.fast_confidence,
        baseline = diagnostic.reliability_baseline,
        div      = diagnostic.divergence,
    )
}

/// Build the user message for a write-time prospective scenario generation call.
///
/// Instructs the model to generate 3–5 future queries that the inserted belief
/// would help answer.  The `generate_scenarios` tool forces structured output.
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

/// JSON schema for the `generate_scenarios` tool.
///
/// Using `tool_choice: {{ type: "any" }}` forces the model to call this tool
/// rather than returning unstructured text.
pub fn generate_scenarios_tool_schema() -> Value {
    json!({
        "name": "generate_scenarios",
        "description": "Return prospective query scenarios for the inserted belief.",
        "input_schema": {
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
    })
}

/// JSON schema for the `report_confidence` tool.
///
/// Using `tool_choice: { type: "any" }` in the request forces the model to
/// invoke this tool rather than returning unstructured text.
pub fn report_confidence_tool_schema() -> Value {
    json!({
        "name": "report_confidence",
        "description": "Report the recalibrated confidence and reasoning for the belief.",
        "input_schema": {
            "type": "object",
            "properties": {
                "revised_confidence": {
                    "type": "number",
                    "minimum": 0.0,
                    "maximum": 1.0,
                    "description": "Recalibrated confidence in [0, 1]."
                },
                "reasoning": {
                    "type": "string",
                    "description": "Brief chain-of-thought justifying the recalibration."
                }
            },
            "required": ["revised_confidence", "reasoning"]
        }
    })
}
