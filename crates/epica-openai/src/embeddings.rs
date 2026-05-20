//! OpenAI-compatible embedding backend for Epica's [`EmbeddingProvider`].
//!
//! Closes TD-P8-001: the K\*6 semantic equivalence check shipped in Sprint 1
//! ships with a `NullEmbeddingProvider` by default and could not be wired to
//! a real backend without the user maintaining the cache by hand. This
//! module is the first concrete backend.
//!
//! ## Wire format
//!
//! Speaks the OpenAI Embeddings REST shape (`POST /v1/embeddings`), which is
//! the de-facto interchange format also accepted by Voyage AI, Together AI,
//! Fireworks, and self-hosted text-embeddings-inference servers. To target a
//! non-OpenAI provider, override [`OpenAiEmbeddingConfig::base_url`] /
//! [`OpenAiEmbeddingConfig::model`] — no code change required.
//!
//! ## Hot path vs warm path
//!
//! The trait [`EmbeddingProvider`] mandates a *sync* `embed_cached` so that
//! `BeliefQuad::check_contradiction` stays sync (see Sprint 1 design notes).
//! The trade-off: actually computing an embedding requires the network and is
//! therefore async. We resolve this by splitting the API:
//!
//! - [`embed_cached`](EmbeddingProvider::embed_cached): sync read against the
//!   in-memory `RwLock<HashMap<String, Vec<f32>>>`. Returns `None` on miss.
//! - [`warm_async`](OpenAiEmbeddingProvider::warm_async): explicit async
//!   helper. Callers running inside a Tokio runtime `await` it to populate
//!   the cache before insertion / revision.
//! - [`warm`](EmbeddingProvider::warm): trait-required sync method. Drains
//!   missing texts into an internal `pending` queue; the next `warm_async`
//!   call processes it. Callers without a Tokio runtime can therefore still
//!   declare intent ("I'll need these later") even if they can't await.
//!
//! ## Retry & batching
//!
//! Retry policy mirrors `OpenAiLlmClient`: 3 attempts, exponential backoff
//! with deterministic jitter, retries on 429 / 5xx / network. Calls are
//! batched up to [`OpenAiEmbeddingConfig::batch_size`] inputs per request
//! (default 64) — OpenAI's embeddings endpoint accepts batches natively, so
//! 200 paraphrases cost 4 requests, not 200.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use serde::Deserialize;
use serde_json::json;
use tracing::{debug, instrument, warn};

use epica_core::EmbeddingProvider;
use epica_runtime::{EmbedError, Embedder};

use crate::config::OpenAiConfig;

// ── Configuration ────────────────────────────────────────────────────────────

/// Configuration for [`OpenAiEmbeddingProvider`].
///
/// Reuses [`OpenAiConfig`] for the API key + base URL so a single env var
/// (`OPENAI_API_KEY`) covers both LLM and embedding usage in typical
/// deployments.
#[derive(Debug, Clone)]
pub struct OpenAiEmbeddingConfig {
    /// Underlying connection config (api_key, base_url).
    pub openai: OpenAiConfig,
    /// Embedding model. Default: `"text-embedding-3-small"` (1536 dims, low
    /// cost). Override to `"text-embedding-3-large"` for higher quality or
    /// to a Voyage / Together / TEI model name when running through their
    /// OpenAI-compatible endpoint.
    pub model: String,
    /// Maximum input texts per HTTP request. Default: `64` (well under the
    /// upstream 2048-item limit; keeps individual call latency bounded).
    pub batch_size: usize,
}

impl OpenAiEmbeddingConfig {
    /// Build a config from an existing [`OpenAiConfig`] with default model
    /// and batch size.
    pub fn from_openai(openai: OpenAiConfig) -> Self {
        Self {
            openai,
            model: "text-embedding-3-small".into(),
            batch_size: 64,
        }
    }

    /// Load `OPENAI_API_KEY` from the environment with embedding defaults.
    pub fn from_env() -> Result<Self, crate::config::OpenAiConfigError> {
        Ok(Self::from_openai(OpenAiConfig::from_env()?))
    }

    /// Override the embedding model.
    #[must_use]
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }

    /// Override the per-request batch size. Clamped to `[1, 2048]` at use
    /// time.
    #[must_use]
    pub fn with_batch_size(mut self, batch_size: usize) -> Self {
        self.batch_size = batch_size.clamp(1, 2048);
        self
    }
}

// ── Errors ────────────────────────────────────────────────────────────────────

/// Errors from embedding API calls.
///
/// Variant split mirrors [`LlmClientError`][epica_runtime::LlmClientError]:
/// network / 5xx are retryable, 4xx and deserialization are not.
#[derive(Debug, thiserror::Error)]
pub enum OpenAiEmbeddingError {
    /// Network failure or 5xx response. Retryable.
    #[error("network error: {0}")]
    Network(String),

    /// HTTP 4xx other than 429 — bad request, bad key, payload too large.
    /// Not retryable.
    #[error("client error: {0}")]
    ClientError(String),

    /// Response could not be parsed into the expected schema.
    #[error("deserialization failed: {0}")]
    Deserialize(String),

    /// HTTP 429. Retryable after backoff.
    #[error("rate limited")]
    RateLimited,

    /// Upstream returned a different number of embeddings than the number of
    /// inputs sent. Indicates an API contract break.
    #[error("provider returned {got} embeddings for {expected} inputs")]
    Mismatch { expected: usize, got: usize },
}

// ── Provider ─────────────────────────────────────────────────────────────────

const MAX_RETRIES: u32 = 3;
const BASE_DELAY_MS: u64 = 500;
const JITTER_MS: u64 = 150;

/// OpenAI-compatible embedding provider with built-in cache.
///
/// Construct with [`Self::new`] / [`Self::from_env`], pre-populate the cache
/// with [`Self::warm_async`], then attach to a `BeliefQuad`:
///
/// ```rust,no_run
/// # async fn run() -> Result<(), Box<dyn std::error::Error>> {
/// use std::sync::Arc;
/// use epica_core::BeliefQuad;
/// use epica_openai::OpenAiEmbeddingProvider;
///
/// let provider = Arc::new(OpenAiEmbeddingProvider::from_env()?);
/// provider.warm_async(&["the user wants to refactor auth",
///                       "user intent: refactor authentication"]).await?;
///
/// let mut quad = BeliefQuad::new();
/// quad.set_embedding_provider(provider);
/// # Ok(())
/// # }
/// ```
pub struct OpenAiEmbeddingProvider {
    config: OpenAiEmbeddingConfig,
    http: reqwest::Client,
    cache: RwLock<HashMap<String, Vec<f32>>>,
    /// Texts queued by the sync `warm()` for the next `warm_async` call to
    /// drain. Behind a `Mutex` because the sync method has only `&self`.
    pending: Mutex<Vec<String>>,
}

impl OpenAiEmbeddingProvider {
    /// Build a provider from an explicit config.
    pub fn new(config: OpenAiEmbeddingConfig) -> Self {
        Self {
            config,
            http: reqwest::Client::new(),
            cache: RwLock::new(HashMap::new()),
            pending: Mutex::new(Vec::new()),
        }
    }

    /// Build with `OPENAI_API_KEY` from the environment and defaults.
    pub fn from_env() -> Result<Self, crate::config::OpenAiConfigError> {
        Ok(Self::new(OpenAiEmbeddingConfig::from_env()?))
    }

    /// Wrap in an `Arc` — convenience for `BeliefQuad::set_embedding_provider`.
    #[must_use]
    pub fn into_arc(self) -> Arc<Self> {
        Arc::new(self)
    }

    /// Number of entries currently cached. Test / diagnostic helper.
    pub fn cached_len(&self) -> usize {
        self.cache.read().map(|m| m.len()).unwrap_or(0)
    }

    /// Insert an embedding directly into the cache. Used by tests and by
    /// callers that obtained the vector out-of-band.
    pub fn insert_cached(&self, text: impl Into<String>, embedding: Vec<f32>) {
        if let Ok(mut g) = self.cache.write() {
            g.insert(text.into(), embedding);
        }
    }

    /// Async path: embed any of `texts` that are not already cached, then
    /// store them. Returns the number of new embeddings fetched (0 when the
    /// cache was already warm). Also drains any texts queued by the sync
    /// `warm()` trait method.
    pub async fn warm_async(&self, texts: &[&str]) -> Result<usize, OpenAiEmbeddingError> {
        // Pull pending texts queued via the sync trait API.
        let mut missing: Vec<String> = {
            let cache = self.cache.read().map_err(|_| {
                OpenAiEmbeddingError::Network("cache lock poisoned".into())
            })?;
            let pending = self.pending.lock().map_err(|_| {
                OpenAiEmbeddingError::Network("pending lock poisoned".into())
            })?;
            let mut out = Vec::new();
            for t in pending.iter() {
                if !cache.contains_key(t) && !out.contains(t) {
                    out.push(t.clone());
                }
            }
            for t in texts {
                if !cache.contains_key(*t) && !out.iter().any(|p| p == *t) {
                    out.push((*t).to_string());
                }
            }
            out
        };
        // Pending is now consumed.
        if let Ok(mut p) = self.pending.lock() {
            p.clear();
        }

        if missing.is_empty() {
            return Ok(0);
        }

        let mut fetched = 0usize;
        for chunk in missing.chunks_mut(self.config.batch_size) {
            let inputs: Vec<&str> = chunk.iter().map(String::as_str).collect();
            let vectors = self.fetch_with_retry(&inputs).await?;
            if let Ok(mut cache) = self.cache.write() {
                for (text, vec) in chunk.iter().zip(vectors.into_iter()) {
                    cache.insert(text.clone(), vec);
                    fetched += 1;
                }
            }
        }
        Ok(fetched)
    }

    async fn fetch_with_retry(
        &self,
        inputs: &[&str],
    ) -> Result<Vec<Vec<f32>>, OpenAiEmbeddingError> {
        let mut last_err =
            OpenAiEmbeddingError::Network("no attempts made".into());

        for attempt in 0..MAX_RETRIES {
            if attempt > 0 {
                let delay = BASE_DELAY_MS * (1u64 << (attempt - 1));
                let jitter = (attempt as u64) * (JITTER_MS / MAX_RETRIES as u64);
                let total = std::time::Duration::from_millis(delay + jitter);
                warn!(attempt, delay_ms = total.as_millis(),
                    "embeddings: retrying after transient error");
                tokio::time::sleep(total).await;
            }
            match self.try_fetch(inputs).await {
                Ok(v) => return Ok(v),
                Err(e) if is_retryable(&e) => {
                    warn!(attempt, error = %e,
                        "embeddings: transient error, will retry");
                    last_err = e;
                }
                Err(e) => return Err(e),
            }
        }
        Err(last_err)
    }

    #[instrument(skip(self, inputs), fields(n = inputs.len()))]
    async fn try_fetch(
        &self,
        inputs: &[&str],
    ) -> Result<Vec<Vec<f32>>, OpenAiEmbeddingError> {
        let url = format!("{}/v1/embeddings", self.config.openai.base_url);
        let body = json!({
            "model": self.config.model,
            "input": inputs,
        });
        debug!(url = %url, model = %self.config.model, "embeddings: sending request");

        let response = self
            .http
            .post(&url)
            .bearer_auth(&self.config.openai.api_key)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| OpenAiEmbeddingError::Network(e.to_string()))?;

        let status = response.status();
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(OpenAiEmbeddingError::RateLimited);
        }
        if status.is_client_error() {
            let text = response.text().await.unwrap_or_default();
            return Err(OpenAiEmbeddingError::ClientError(format!(
                "HTTP {status}: {text}"
            )));
        }
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(OpenAiEmbeddingError::Network(format!(
                "HTTP {status}: {text}"
            )));
        }

        let parsed: EmbeddingsResponse = response
            .json()
            .await
            .map_err(|e| OpenAiEmbeddingError::Deserialize(e.to_string()))?;
        if parsed.data.len() != inputs.len() {
            return Err(OpenAiEmbeddingError::Mismatch {
                expected: inputs.len(),
                got: parsed.data.len(),
            });
        }
        // The API does not guarantee ordering, but documents `index` per
        // entry. Sort by index to be safe.
        let mut entries = parsed.data;
        entries.sort_by_key(|e| e.index);
        Ok(entries.into_iter().map(|e| e.embedding).collect())
    }
}

/// Bridges the sync K\*6 path ([`EmbeddingProvider`]) and the async
/// prospective-indexing path ([`Embedder`]) over a single cache.
///
/// `epica-runtime`'s [`Embedder`] trait is async because production embedding
/// backends are network-bound. `epica-core`'s [`EmbeddingProvider`] is sync
/// because it's called on the AGM hot path. The same `OpenAiEmbeddingProvider`
/// satisfies both by:
/// - serving `embed_cached` from the in-memory `HashMap` (sync, never blocks),
/// - serving `embed` by short-circuiting to the cache on hit, otherwise
///   awaiting `warm_async` to populate and re-reading.
///
/// One provider instance can therefore be `Arc`-shared between
/// `BeliefQuad::set_embedding_provider` (K\*6) and
/// `BeliefRuntime::with_embedder` (prospective). The cache is shared, so the
/// LLM-generated scenarios from prospective indexing automatically warm the
/// K\*6 path for those exact strings.
///
/// OpenAI's `text-embedding-3-*` models return unit-norm vectors, so the L2
/// normalisation contract documented on `Embedder::embed` is upheld upstream
/// — no client-side renormalisation is performed.
#[async_trait::async_trait]
impl Embedder for OpenAiEmbeddingProvider {
    async fn embed(&self, text: &str) -> Result<Vec<f32>, EmbedError> {
        if let Some(v) = self.embed_cached(text) {
            return Ok(v);
        }
        self.warm_async(&[text]).await.map_err(map_embedding_error)?;
        self.embed_cached(text).ok_or_else(|| {
            EmbedError::Model(
                "embedding missing from cache immediately after warm_async — provider returned 0 entries".into(),
            )
        })
    }
}

/// Map an `OpenAiEmbeddingError` onto the runtime's narrower [`EmbedError`].
///
/// `RateLimited` is mapped to `Network` (retryable from the caller's
/// perspective). `ClientError` / `Deserialize` / `Mismatch` are mapped to
/// `Model` (caller should not retry — they signal a contract or auth issue).
fn map_embedding_error(e: OpenAiEmbeddingError) -> EmbedError {
    match e {
        OpenAiEmbeddingError::Network(s) => EmbedError::Network(s),
        OpenAiEmbeddingError::RateLimited => EmbedError::Network("rate limited".into()),
        OpenAiEmbeddingError::ClientError(s) => EmbedError::Model(s),
        OpenAiEmbeddingError::Deserialize(s) => EmbedError::Model(s),
        OpenAiEmbeddingError::Mismatch { expected, got } => EmbedError::Model(format!(
            "provider returned {got} embeddings for {expected} inputs"
        )),
    }
}

impl EmbeddingProvider for OpenAiEmbeddingProvider {
    fn embed_cached(&self, text: &str) -> Option<Vec<f32>> {
        self.cache.read().ok()?.get(text).cloned()
    }

    fn warm(&self, texts: &[&str]) {
        // Sync method: we cannot block on the network from here without
        // assuming a Tokio runtime. Queue intent and let `warm_async`
        // pick it up. Texts already cached are dropped immediately.
        let cache_snapshot = match self.cache.read() {
            Ok(g) => g,
            Err(_) => return,
        };
        if let Ok(mut p) = self.pending.lock() {
            for t in texts {
                if !cache_snapshot.contains_key(*t) {
                    let owned = (*t).to_string();
                    if !p.contains(&owned) {
                        p.push(owned);
                    }
                }
            }
        }
    }
}

fn is_retryable(e: &OpenAiEmbeddingError) -> bool {
    matches!(
        e,
        OpenAiEmbeddingError::RateLimited | OpenAiEmbeddingError::Network(_)
    )
}

impl std::fmt::Debug for OpenAiEmbeddingProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenAiEmbeddingProvider")
            .field("model", &self.config.model)
            .field("base_url", &self.config.openai.base_url)
            .field("cached_len", &self.cached_len())
            .finish()
    }
}

// ── Wire types ───────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct EmbeddingsResponse {
    data: Vec<EmbeddingDatum>,
}

#[derive(Debug, Deserialize)]
struct EmbeddingDatum {
    embedding: Vec<f32>,
    index: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warm_queues_missing_texts() {
        let cfg = OpenAiEmbeddingConfig::from_openai(OpenAiConfig::new("test-key"));
        let p = OpenAiEmbeddingProvider::new(cfg);
        // Pre-cache one entry; the other should land in pending.
        p.insert_cached("already", vec![1.0, 0.0]);
        p.warm(&["already", "new", "another"]);
        let pending = p.pending.lock().unwrap();
        assert_eq!(pending.len(), 2);
        assert!(pending.contains(&"new".to_string()));
        assert!(pending.contains(&"another".to_string()));
        assert!(!pending.contains(&"already".to_string()));
    }

    #[test]
    fn embed_cached_hits_and_misses() {
        let cfg = OpenAiEmbeddingConfig::from_openai(OpenAiConfig::new("test-key"));
        let p = OpenAiEmbeddingProvider::new(cfg);
        assert!(p.embed_cached("x").is_none());
        p.insert_cached("x", vec![0.1, 0.2, 0.3]);
        assert_eq!(p.embed_cached("x"), Some(vec![0.1, 0.2, 0.3]));
    }

    #[test]
    fn config_batch_size_is_clamped() {
        let cfg = OpenAiEmbeddingConfig::from_openai(OpenAiConfig::new("k"))
            .with_batch_size(0);
        assert_eq!(cfg.batch_size, 1);
        let cfg = OpenAiEmbeddingConfig::from_openai(OpenAiConfig::new("k"))
            .with_batch_size(99_999);
        assert_eq!(cfg.batch_size, 2048);
    }

    #[test]
    fn is_retryable_classifies_correctly() {
        assert!(is_retryable(&OpenAiEmbeddingError::RateLimited));
        assert!(is_retryable(&OpenAiEmbeddingError::Network("x".into())));
        assert!(!is_retryable(&OpenAiEmbeddingError::ClientError("x".into())));
        assert!(!is_retryable(&OpenAiEmbeddingError::Deserialize("x".into())));
        assert!(!is_retryable(&OpenAiEmbeddingError::Mismatch {
            expected: 2,
            got: 1
        }));
    }
}
