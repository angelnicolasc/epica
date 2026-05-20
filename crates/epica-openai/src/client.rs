//! [`OpenAiLlmClient`]: [`LlmClient`] implementation via the OpenAI Chat
//! Completions API with forced function calling.

use std::sync::Arc;

use serde::Deserialize;
use serde_json::json;
use tracing::{debug, instrument, warn};

use epica_runtime::{
    DiagnosticSignal, LlmClient, LlmClientError, ProspectiveClient,
    ProspectiveClientError, RawProspectiveScenario, System2Result,
};

use crate::config::{OpenAiConfig, OpenAiConfigError};
use crate::prompt::{build_prospective_message, generate_scenarios_function_schema};

/// OpenAI Chat Completions API client implementing System 2 reflection.
///
/// # Example
///
/// ```rust,no_run
/// use epica_openai::OpenAiLlmClient;
/// use epica_runtime::BeliefRuntime;
/// use epica_core::BeliefQuad;
/// use std::sync::Arc;
///
/// # async fn run() -> Result<(), Box<dyn std::error::Error>> {
/// let client = OpenAiLlmClient::from_env()?;
/// let rt = BeliefRuntime::new(BeliefQuad::new(), 0.5, 10, 1.0)
///     .with_llm_client(Arc::new(client));
/// # Ok(())
/// # }
/// ```
pub struct OpenAiLlmClient {
    config: OpenAiConfig,
    http: reqwest::Client,
}

impl OpenAiLlmClient {
    /// Create a client with an explicit configuration.
    pub fn new(config: OpenAiConfig) -> Self {
        Self {
            config,
            http: reqwest::Client::new(),
        }
    }

    /// Load configuration from the `OPENAI_API_KEY` environment variable.
    ///
    /// # Errors
    /// Returns [`OpenAiConfigError::MissingApiKey`] when the variable is absent.
    pub fn from_env() -> Result<Self, OpenAiConfigError> {
        Ok(Self::new(OpenAiConfig::from_env()?))
    }

    /// Wrap in an `Arc` — convenience for passing to `BeliefRuntime::with_llm_client`.
    #[must_use]
    pub fn into_arc(self) -> Arc<Self> {
        Arc::new(self)
    }
}

/// Maximum number of retry attempts for transient LLM errors.
const MAX_RETRIES: u32 = 3;
/// Base delay (ms) for exponential backoff. Doubles on each retry.
const BASE_DELAY_MS: u64 = 500;
/// Maximum jitter (ms) added to each backoff to avoid thundering herd.
const JITTER_MS: u64 = 150;

#[async_trait::async_trait]
impl LlmClient for OpenAiLlmClient {
    #[instrument(skip(self), fields(key = %diagnostic.belief_key, fast = diagnostic.fast_confidence))]
    async fn reflect(
        &self,
        diagnostic: &DiagnosticSignal,
    ) -> Result<System2Result, LlmClientError> {
        let mut last_err = LlmClientError::Network("no attempts made".into());

        for attempt in 0..MAX_RETRIES {
            if attempt > 0 {
                let delay = BASE_DELAY_MS * (1u64 << (attempt - 1));
                let jitter = (attempt as u64) * (JITTER_MS / MAX_RETRIES as u64);
                let total = std::time::Duration::from_millis(delay + jitter);
                warn!(attempt, delay_ms = total.as_millis(), "System 2: retrying after transient error");
                tokio::time::sleep(total).await;
            }

            match self.try_reflect(diagnostic).await {
                Ok(result) => return Ok(result),
                Err(e) if is_retryable(&e) => {
                    warn!(attempt, error = %e, "System 2: transient error, will retry");
                    last_err = e;
                }
                Err(e) => return Err(e),
            }
        }

        Err(last_err)
    }
}

/// Returns `true` for errors that are worth retrying (rate limits, transient server errors).
fn is_retryable(e: &LlmClientError) -> bool {
    matches!(e, LlmClientError::RateLimited | LlmClientError::Network(_))
}

impl OpenAiLlmClient {
    async fn try_reflect(
        &self,
        diagnostic: &DiagnosticSignal,
    ) -> Result<System2Result, LlmClientError> {
        let url = format!("{}/v1/chat/completions", self.config.base_url);
        let body = json!({
            "model": self.config.model,
            "max_tokens": self.config.max_tokens,
            "messages": [{
                "role": "user",
                "content": build_system2_message(diagnostic),
            }],
            "tools": [report_confidence_function_schema()],
            // Force the model to call `report_confidence` rather than emit text.
            "tool_choice": {
                "type": "function",
                "function": { "name": "report_confidence" }
            },
        });

        debug!(url = %url, "System 2: sending OpenAI reflection request");

        let response = self
            .http
            .post(&url)
            .bearer_auth(&self.config.api_key)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| LlmClientError::Network(e.to_string()))?;

        let status = response.status();
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            warn!("System 2: OpenAI rate limit hit");
            return Err(LlmClientError::RateLimited);
        }
        if status.is_client_error() {
            // 4xx other than 429: bad key, bad request, payload too large, etc.
            // Retrying will not help.
            let text = response.text().await.unwrap_or_default();
            return Err(LlmClientError::ClientError(format!("HTTP {status}: {text}")));
        }
        if !status.is_success() {
            // 5xx and other unexpected responses are treated as transient.
            let text = response.text().await.unwrap_or_default();
            return Err(LlmClientError::Network(format!("HTTP {status}: {text}")));
        }

        let body: ChatCompletionResponse = response
            .json()
            .await
            .map_err(|e| LlmClientError::Deserialize(e.to_string()))?;

        let call = body
            .choices
            .first()
            .and_then(|c| c.message.tool_calls.first())
            .filter(|c| c.function.name == "report_confidence")
            .ok_or_else(|| {
                LlmClientError::Deserialize(
                    "no report_confidence tool_call in OpenAI response".into(),
                )
            })?;

        // The OpenAI API returns `arguments` as a JSON-encoded string, not an object.
        let parsed: ConfidenceArgs = serde_json::from_str(&call.function.arguments)
            .map_err(|e| LlmClientError::Deserialize(e.to_string()))?;

        let revised_confidence = parsed.revised_confidence.clamp(0.0, 1.0);
        debug!(revised = revised_confidence, "System 2: OpenAI reflection complete");

        Ok(System2Result {
            revised_confidence,
            reasoning: parsed.reasoning,
        })
    }
}

// ── Phase 4: ProspectiveClient implementation ─────────────────────────────────

/// Model used for write-time prospective indexing.
///
/// Mirrors the Anthropic side's choice of Haiku for the same role: scenario
/// generation is high-volume and called synchronously on every
/// `insert_belief()` with `prospect = true`, so a fast/cheap model dominates.
/// `gpt-4o-mini` is the production-quality equivalent in the OpenAI catalog.
const PROSPECTIVE_MODEL: &str = "gpt-4o-mini";

#[async_trait::async_trait]
impl ProspectiveClient for OpenAiLlmClient {
    #[instrument(skip(self), fields(key = %belief_key))]
    async fn generate_scenarios(
        &self,
        belief_key: &str,
        belief_value_repr: &str,
    ) -> Result<Vec<RawProspectiveScenario>, ProspectiveClientError> {
        let url = format!("{}/v1/chat/completions", self.config.base_url);
        let body = json!({
            "model": PROSPECTIVE_MODEL,
            "max_tokens": 512,
            "messages": [{
                "role": "user",
                "content": build_prospective_message(belief_key, belief_value_repr),
            }],
            "tools": [generate_scenarios_function_schema()],
            "tool_choice": {
                "type": "function",
                "function": { "name": "generate_scenarios" }
            },
        });

        debug!(url = %url, "prospective: sending OpenAI scenario generation request");

        let response = self
            .http
            .post(&url)
            .bearer_auth(&self.config.api_key)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| ProspectiveClientError::Network(e.to_string()))?;

        let status = response.status();
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            warn!("prospective: OpenAI rate limit hit");
            return Err(ProspectiveClientError::RateLimited);
        }
        // `ProspectiveClientError` does not (yet) split 4xx/5xx the way
        // `LlmClientError` does; both flow through `Network`. A future
        // revision can widen the enum if write-time retry policy diverges
        // from System 2's.
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(ProspectiveClientError::Network(format!(
                "HTTP {status}: {text}"
            )));
        }

        let body: ChatCompletionResponse = response
            .json()
            .await
            .map_err(|e| ProspectiveClientError::Deserialize(e.to_string()))?;

        let call = body
            .choices
            .first()
            .and_then(|c| c.message.tool_calls.first())
            .filter(|c| c.function.name == "generate_scenarios")
            .ok_or_else(|| {
                ProspectiveClientError::Deserialize(
                    "no generate_scenarios tool_call in OpenAI response".into(),
                )
            })?;

        let parsed: ScenariosArgs = serde_json::from_str(&call.function.arguments)
            .map_err(|e| ProspectiveClientError::Deserialize(e.to_string()))?;

        let scenarios: Vec<RawProspectiveScenario> = parsed
            .scenarios
            .into_iter()
            .map(|s| RawProspectiveScenario {
                scenario: s.query,
                // Causal-event extraction from free-form scenarios is the
                // same out-of-scope concern as on the Anthropic side; left
                // empty until a dedicated extraction pass lands.
                causal_events: vec![],
            })
            .collect();

        let scenario_count = scenarios.len();
        debug!("prospective: OpenAI generated {scenario_count} scenarios for {belief_key}");

        Ok(scenarios)
    }
}

#[derive(Deserialize)]
struct ScenariosArgs {
    scenarios: Vec<ScenarioItem>,
}

#[derive(Deserialize)]
struct ScenarioItem {
    query: String,
}

// ── Prompt & schema ──────────────────────────────────────────────────────────

/// Build the user-message content for a System 2 reflection call.
///
/// The prompt is intentionally provider-agnostic — same intent, same epistemic
/// framing as the Anthropic prompt — so cross-provider comparisons of System 2
/// quality reflect model differences, not prompt differences.
fn build_system2_message(diagnostic: &DiagnosticSignal) -> String {
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

/// OpenAI `tools[]` schema for the `report_confidence` function.
fn report_confidence_function_schema() -> serde_json::Value {
    json!({
        "type": "function",
        "function": {
            "name": "report_confidence",
            "description": "Report the recalibrated confidence and reasoning for the belief.",
            "parameters": {
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
        }
    })
}

// ── Response wire types ──────────────────────────────────────────────────────

#[derive(Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: Message,
}

#[derive(Deserialize)]
struct Message {
    #[serde(default)]
    tool_calls: Vec<ToolCall>,
}

#[derive(Deserialize)]
struct ToolCall {
    function: ToolFunction,
}

#[derive(Deserialize)]
struct ToolFunction {
    name: String,
    arguments: String,
}

#[derive(Deserialize)]
struct ConfidenceArgs {
    revised_confidence: f32,
    reasoning: String,
}
