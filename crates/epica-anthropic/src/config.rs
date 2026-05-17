//! Configuration for `AnthropicLlmClient`.

use std::env;

/// Configuration for the Anthropic Messages API client.
///
/// Build via the builder methods or load from environment variables with
/// [`AnthropicConfig::from_env()`].
#[derive(Debug, Clone)]
pub struct AnthropicConfig {
    /// Anthropic API key.  Required.
    pub api_key: String,

    /// Model ID used for System 2 reflection calls.
    /// Default: `"claude-sonnet-4-6"`.
    pub model: String,

    /// Maximum tokens in the model response.  Default: `512`.
    pub max_tokens: u32,

    /// Base URL for the Anthropic API.  Default: `"https://api.anthropic.com"`.
    pub base_url: String,
}

impl AnthropicConfig {
    /// Create a config with an explicit API key and defaults for other fields.
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            model: "claude-sonnet-4-6".into(),
            max_tokens: 512,
            base_url: "https://api.anthropic.com".into(),
        }
    }

    /// Load `ANTHROPIC_API_KEY` from the environment.
    ///
    /// # Errors
    /// Returns [`AnthropicConfigError::MissingApiKey`] when the variable is absent.
    pub fn from_env() -> Result<Self, AnthropicConfigError> {
        let api_key = env::var("ANTHROPIC_API_KEY")
            .map_err(|_| AnthropicConfigError::MissingApiKey)?;
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

    /// Override the base URL (useful for proxies or local endpoints).
    #[must_use]
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }
}

/// Errors constructing an [`AnthropicConfig`].
#[derive(Debug, thiserror::Error)]
pub enum AnthropicConfigError {
    /// `ANTHROPIC_API_KEY` environment variable is not set.
    #[error("environment variable ANTHROPIC_API_KEY is not set")]
    MissingApiKey,
}
