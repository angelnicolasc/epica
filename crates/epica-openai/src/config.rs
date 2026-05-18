//! Configuration for [`crate::OpenAiLlmClient`].

use std::env;

/// Configuration for the OpenAI Chat Completions API client.
///
/// Build via the builder methods or load from environment variables with
/// [`OpenAiConfig::from_env()`].
#[derive(Debug, Clone)]
pub struct OpenAiConfig {
    /// OpenAI API key. Required.
    pub api_key: String,

    /// Model ID used for System 2 reflection calls.
    /// Default: `"gpt-4o-mini"` — chosen to balance per-call latency and cost.
    pub model: String,

    /// Maximum tokens in the model response. Default: `512`.
    pub max_tokens: u32,

    /// Base URL for the OpenAI API. Default: `"https://api.openai.com"`.
    /// Override for self-hosted or Azure OpenAI deployments.
    pub base_url: String,
}

impl OpenAiConfig {
    /// Create a config with an explicit API key and defaults for other fields.
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            model: "gpt-4o-mini".into(),
            max_tokens: 512,
            base_url: "https://api.openai.com".into(),
        }
    }

    /// Load `OPENAI_API_KEY` from the environment.
    ///
    /// # Errors
    /// Returns [`OpenAiConfigError::MissingApiKey`] when the variable is absent.
    pub fn from_env() -> Result<Self, OpenAiConfigError> {
        let api_key = env::var("OPENAI_API_KEY").map_err(|_| OpenAiConfigError::MissingApiKey)?;
        Ok(Self::new(api_key))
    }

    /// Override the model ID.
    #[must_use]
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }

    /// Override the max-tokens limit.
    #[must_use]
    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = max_tokens;
        self
    }

    /// Override the base URL (useful for Azure OpenAI, proxies, or local mocks).
    #[must_use]
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }
}

/// Errors constructing an [`OpenAiConfig`].
#[derive(Debug, thiserror::Error)]
pub enum OpenAiConfigError {
    /// `OPENAI_API_KEY` environment variable is not set.
    #[error("environment variable OPENAI_API_KEY is not set")]
    MissingApiKey,
}
