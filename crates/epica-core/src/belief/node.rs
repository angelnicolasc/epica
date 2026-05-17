use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::id::BeliefId;

/// A single unit of agent knowledge within the [`BeliefQuad`][crate::BeliefQuad].
///
/// Every `BeliefNode` exists simultaneously in all four orthogonal graphs
/// (semantic, temporal, causal, entity). Removing a node from the quad removes
/// its projections from all graphs.
///
/// # Epistemic taxonomy
///
/// `provenance` follows the pramāṇa taxonomy:
/// - `pratyakṣa` — direct perception (tool result)
/// - `anumāna`   — inference (LLM output)
/// - `śabda`     — testimony (user statement)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BeliefNode {
    /// Stable identifier — assigned by the [`BeliefQuad`][crate::BeliefQuad] on insert.
    pub id: BeliefId,

    /// Domain-scoped key, e.g. `"user_intent"` or `"file_tree"`.
    /// Keys must be unique within a quad; revision by key is the primary update path.
    pub key: String,

    /// The actual belief content.
    pub value: BeliefValue,

    /// Where this belief came from.
    pub provenance: Provenance,

    /// Creation time as milliseconds since UNIX_EPOCH.
    pub created_at_ms: u64,

    /// Optional time-to-live in milliseconds.
    /// `None` means the belief never decays automatically.
    pub ttl_ms: Option<u64>,

    /// Fast-path confidence — maintained by System 1 (Noisy-OR propagation).
    /// Always in `[0.0, 1.0]`.
    pub fast_confidence: f32,

    /// Slow-path confidence — set by System 2 when activated.
    /// `None` until System 2 has reflected on this belief.
    pub slow_confidence: Option<f32>,

    /// Divergence threshold τ that activates System 2 for this belief.
    /// Paper AUQ reports τ ≈ 0.15 as optimal on ALFWorld.
    pub reflection_threshold: f32,

    /// Whether write-time prospective indexing is enabled for this belief.
    ///
    /// When `true`, `BeliefRuntime::insert_belief()` calls the prospective
    /// client to generate future query scenarios and embed them at write time
    /// (Kumiho arXiv:2603.17244, §4.2). Set via `#[belief(prospect = true)]`.
    #[serde(default)]
    pub prospect: bool,
}

impl BeliefNode {
    /// Construct a new node with System 1 confidence only.
    ///
    /// `created_at_ms` is set to the current wall-clock time.
    /// `reflection_threshold` defaults to [`DEFAULT_REFLECTION_THRESHOLD`][crate::belief::confidence::DEFAULT_REFLECTION_THRESHOLD].
    pub fn new(key: impl Into<String>, value: BeliefValue, provenance: Provenance, confidence: f32) -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        Self {
            id: BeliefId::default(),
            key: key.into(),
            value,
            provenance,
            created_at_ms: now_ms,
            ttl_ms: None,
            fast_confidence: confidence.clamp(0.0, 1.0),
            slow_confidence: None,
            reflection_threshold: crate::belief::confidence::DEFAULT_REFLECTION_THRESHOLD,
            prospect: false,
        }
    }

    /// Builder: set a TTL after which the belief decays.
    #[must_use]
    pub fn with_ttl_ms(mut self, ttl_ms: u64) -> Self {
        self.ttl_ms = Some(ttl_ms);
        self
    }

    /// Builder: override the reflection threshold τ.
    #[must_use]
    pub fn with_reflection_threshold(mut self, tau: f32) -> Self {
        self.reflection_threshold = tau.clamp(0.0, 1.0);
        self
    }

    /// Builder: enable or disable write-time prospective indexing.
    #[must_use]
    pub fn with_prospect(mut self, v: bool) -> Self {
        self.prospect = v;
        self
    }

    /// Returns true if the belief has passed its TTL.
    pub fn is_expired(&self, current_ms: u64) -> bool {
        match self.ttl_ms {
            None => false,
            Some(ttl) => current_ms.saturating_sub(self.created_at_ms) >= ttl,
        }
    }
}

/// The content of a belief.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum BeliefValue {
    /// A deterministic fact from a tool result (pratyakṣa — direct perception).
    Deterministic(serde_json::Value),

    /// An LLM inference (anumāna — inference).
    Inferred(serde_json::Value),

    /// A user assertion (śabda — testimony).
    Asserted(String),

    /// A pointer to another belief node (aliasing / normalization).
    Reference(BeliefId),
}

/// The origin of a belief.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum Provenance {
    ToolResult {
        tool: String,
        call_id: Uuid,
    },
    LlmInference {
        model: String,
        call_id: Uuid,
        prompt_hash: u64,
    },
    UserStatement {
        turn: u32,
    },
    RuntimeInference {
        derived_from: Vec<BeliefId>,
    },
}
