//! `AnthropicLlmClient`: `LlmClient` implementation via the Anthropic Messages API.
//!
//! Uses `tool_use` with `tool_choice: { type: "any" }` to obtain structured
//! JSON output for System 2 confidence recalibration — no regex, no text
//! parsing, no ambiguous free-form response.

use std::sync::Arc;

use serde::Deserialize;
use serde_json::json;
use tracing::{debug, instrument, warn};

use epica_runtime::{
    DiagnosticSignal, LlmClient, LlmClientError, System2Result,
    ProspectiveClient, ProspectiveClientError, RawProspectiveScenario,
};

use crate::{
    config::{AnthropicConfig, AnthropicConfigError},
    prompt::{
        build_prospective_message, build_system2_message,
        generate_scenarios_tool_schema, report_confidence_tool_schema,
    },
};

/// Anthropic Messages API client implementing System 2 reflection.
///
/// # Example
///
/// ```rust,no_run
/// use epica_anthropic::AnthropicLlmClient;
/// use epica_runtime::BeliefRuntime;
/// use epica_core::BeliefQuad;
/// use std::sync::Arc;
///
/// # async fn run() -> Result<(), Box<dyn std::error::Error>> {
/// let client = AnthropicLlmClient::from_env()?;
/// let rt = BeliefRuntime::new(BeliefQuad::new(), 0.5, 10, 1.0)
///     .with_llm_client(Arc::new(client));
/// # Ok(())
/// # }
/// ```
pub struct AnthropicLlmClient {
    config: AnthropicConfig,
    http: reqwest::Client,
}

impl AnthropicLlmClient {
    /// Create a client with an explicit configuration.
    pub fn new(config: AnthropicConfig) -> Self {
        Self {
            config,
            http: reqwest::Client::new(),
        }
    }

    /// Load configuration from the `ANTHROPIC_API_KEY` environment variable.
    ///
    /// # Errors
    /// Returns `AnthropicConfigError::MissingApiKey` if the variable is absent.
    pub fn from_env() -> Result<Self, AnthropicConfigError> {
        Ok(Self::new(AnthropicConfig::from_env()?))
    }

    /// Wrap in an `Arc` — convenience for passing to `BeliefRuntime::with_llm_client`.
    pub fn into_arc(self) -> Arc<Self> {
        Arc::new(self)
    }
}

#[async_trait::async_trait]
impl LlmClient for AnthropicLlmClient {
    #[instrument(skip(self), fields(key = %diagnostic.belief_key, fast = diagnostic.fast_confidence))]
    async fn reflect(
        &self,
        diagnostic: &DiagnosticSignal,
    ) -> Result<System2Result, LlmClientError> {
        let url = format!("{}/v1/messages", self.config.base_url);
        let body = json!({
            "model": self.config.model,
            "max_tokens": self.config.max_tokens,
            "tools": [report_confidence_tool_schema()],
            "tool_choice": { "type": "any" },
            "messages": [{
                "role": "user",
                "content": build_system2_message(diagnostic)
            }]
        });

        debug!(url = %url, "System 2: sending reflection request");

        let response = self.http
            .post(&url)
            .header("x-api-key", &self.config.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| LlmClientError::Network(e.to_string()))?;

        let status = response.status();
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            warn!("System 2: Anthropic rate limit hit");
            return Err(LlmClientError::RateLimited);
        }
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(LlmClientError::Network(format!("HTTP {status}: {text}")));
        }

        let msg: MessagesResponse = response
            .json()
            .await
            .map_err(|e| LlmClientError::Deserialize(e.to_string()))?;

        // Find the `report_confidence` tool_use block
        let tool_input = msg
            .content
            .iter()
            .find(|c| c.r#type == "tool_use" && c.name.as_deref() == Some("report_confidence"))
            .and_then(|c| c.input.as_ref())
            .ok_or_else(|| {
                LlmClientError::Deserialize("no report_confidence tool_use in response".into())
            })?;

        let parsed: ConfidenceToolInput = serde_json::from_value(tool_input.clone())
            .map_err(|e| LlmClientError::Deserialize(e.to_string()))?;

        let revised_confidence = parsed.revised_confidence.clamp(0.0, 1.0);
        debug!(revised = revised_confidence, "System 2: reflection complete");

        Ok(System2Result {
            revised_confidence,
            reasoning: parsed.reasoning,
        })
    }
}

// ── Phase 4: ProspectiveClient implementation ─────────────────────────────────

/// Haiku model used for write-time prospective indexing.
///
/// Haiku is used (not Sonnet) because scenario generation is cheap, high-volume,
/// and called synchronously on every `insert_belief()` with `prospect = true`.
const HAIKU_MODEL: &str = "claude-haiku-4-5-20251001";

#[async_trait::async_trait]
impl ProspectiveClient for AnthropicLlmClient {
    #[instrument(skip(self), fields(key = %belief_key))]
    async fn generate_scenarios(
        &self,
        belief_key: &str,
        belief_value_repr: &str,
    ) -> Result<Vec<RawProspectiveScenario>, ProspectiveClientError> {
        let url = format!("{}/v1/messages", self.config.base_url);
        let body = json!({
            "model": HAIKU_MODEL,
            "max_tokens": 512,
            "tools": [generate_scenarios_tool_schema()],
            "tool_choice": { "type": "any" },
            "messages": [{
                "role": "user",
                "content": build_prospective_message(belief_key, belief_value_repr)
            }]
        });

        debug!(url = %url, "prospective: sending scenario generation request");

        let response = self.http
            .post(&url)
            .header("x-api-key", &self.config.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| ProspectiveClientError::Network(e.to_string()))?;

        let status = response.status();
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            warn!("prospective: Anthropic rate limit hit");
            return Err(ProspectiveClientError::RateLimited);
        }
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(ProspectiveClientError::Network(format!("HTTP {status}: {text}")));
        }

        let msg: MessagesResponse = response
            .json()
            .await
            .map_err(|e| ProspectiveClientError::Deserialize(e.to_string()))?;

        let tool_input = msg
            .content
            .iter()
            .find(|c| c.r#type == "tool_use" && c.name.as_deref() == Some("generate_scenarios"))
            .and_then(|c| c.input.as_ref())
            .ok_or_else(|| {
                ProspectiveClientError::Deserialize(
                    "no generate_scenarios tool_use in response".into(),
                )
            })?;

        let parsed: ScenariosToolInput = serde_json::from_value(tool_input.clone())
            .map_err(|e| ProspectiveClientError::Deserialize(e.to_string()))?;

        let scenarios: Vec<RawProspectiveScenario> = parsed
            .scenarios
            .into_iter()
            .map(|s| RawProspectiveScenario {
                scenario: s.query,
                causal_events: vec![], // Phase 5: extract from narrative
            })
            .collect();

        let scenario_count = scenarios.len();
        debug!("prospective: generated {scenario_count} scenarios for {belief_key}");

        Ok(scenarios)
    }
}

#[derive(Deserialize)]
struct ScenariosToolInput {
    scenarios: Vec<ScenarioItem>,
}

#[derive(Deserialize)]
struct ScenarioItem {
    query: String,
}

// ── Anthropic API response deserialization ────────────────────────────────────

#[derive(Deserialize)]
struct MessagesResponse {
    content: Vec<ContentBlock>,
}

#[derive(Deserialize)]
struct ContentBlock {
    r#type: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    input: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct ConfidenceToolInput {
    revised_confidence: f32,
    reasoning: String,
}
