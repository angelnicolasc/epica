//! `BeliefRuntime`: orchestrates System 1/2, confidence history, and retrieval.

use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::sync::{Mutex, RwLock};

use epica_contracts::{BehavioralContract, MnemonicSovereignty};
use epica_core::{
    BeliefId, BeliefNode, BeliefQuad, BeliefSnapshot, BeliefValue, EpicaError,
    Provenance, should_activate_system2, DEFAULT_REFLECTION_THRESHOLD,
};

use crate::{
    contract_engine::ContractEngine,
    error::{RuntimeError, RuntimeUpdateResult},
    governance_tracker::GovernanceTracker,
    history::ConfidenceHistory,
    prospective::{Embedder, ProspectiveClient},
    retrieval::compute_score,
    session::{SessionReport, TECE_TARGET},
    system2::token_bucket::TokenBucket,
};

#[cfg(feature = "system2")]
use crate::system2::llm_client::{DiagnosticSignal, LlmClient};

/// Async runtime wrapping [`BeliefQuad`] with dual-process uncertainty
/// containment, confidence history, and multicriteria retrieval.
///
/// # Dual-process model
///
/// - **System 1** (fast): runs on every `update_belief()` call via
///   `BeliefQuad::propagate_system1()`.  O(descendants), synchronous,
///   zero LLM cost.
///
/// - **System 2** (slow): activated when
///   `|fast_confidence − reliability_baseline| > per_node_τ`.  Triggers
///   an `LlmClient::reflect()` call to recalibrate confidence via inverse
///   optimisation (AUQ §3.2, arXiv:2601.15703).  Rate-limited by a token
///   bucket to bound session cost.
pub struct BeliefRuntime {
    pub(crate) quad: Arc<RwLock<BeliefQuad>>,

    /// AUQ §3.2 reliability baseline `b`.  System 2 fires when
    /// `|fast_confidence − reliability_baseline| > node.reflection_threshold`.
    /// Recommended: `0.5`.  Per-node τ defaults to `DEFAULT_REFLECTION_THRESHOLD`.
    pub(crate) reliability_baseline: f32,

    /// Token bucket rate-limiting System 2 LLM calls.
    pub(crate) reflection_budget: Mutex<TokenBucket>,

    /// O(1) key → `BeliefId` index, kept in sync with the quad on every insert.
    pub(crate) key_index: Arc<RwLock<HashMap<String, BeliefId>>>,

    /// Per-session confidence trace for Trajectory-ECE computation.
    pub(crate) confidence_history: Arc<RwLock<ConfidenceHistory>>,

    /// Monotonic counter — number of successful System 2 activations.
    pub(crate) system2_activations: Arc<AtomicU64>,

    /// Monotonic counter — number of times System 2 was skipped due to budget.
    pub(crate) system2_throttled: Arc<AtomicU64>,

    /// Phase 3: behavioral contract enforcement engine.
    pub(crate) contract_engine: Arc<tokio::sync::RwLock<ContractEngine>>,

    /// Phase 3: governance resource tracker (token limits, tool-call caps).
    pub(crate) governance_tracker: GovernanceTracker,

    #[cfg(feature = "system2")]
    pub(crate) llm_client: Option<Arc<dyn LlmClient>>,

    /// Global default τ for System 2 activation, applied to any node whose
    /// `reflection_threshold` has not been overridden via `.with_reflection_threshold()`.
    /// Configurable at runtime via `EPICA_REFLECTION_THRESHOLD` env var.
    pub(crate) default_reflection_threshold: f32,

    /// Phase 4: text embedder for prospective retrieval (TD-001).
    pub(crate) embedder: Option<Arc<dyn Embedder>>,

    /// Phase 4: LLM client for write-time prospective scenario generation (TD-001).
    pub(crate) prospective_client: Option<Arc<dyn ProspectiveClient>>,
}

impl BeliefRuntime {
    /// Create a runtime wrapping an existing quad.
    ///
    /// # Parameters
    /// - `reliability_baseline` — AUQ §3.2 `b`; recommended `0.5`.
    /// - `system2_budget` — initial token bucket capacity.
    /// - `system2_refill_rate` — tokens per second; `0.0` for a fixed budget.
    pub fn new(
        quad: BeliefQuad,
        reliability_baseline: f32,
        system2_budget: u32,
        system2_refill_rate: f32,
    ) -> Self {
        let key_index: HashMap<String, BeliefId> =
            quad.iter().map(|(id, n)| (n.key.clone(), id)).collect();
        Self {
            quad: Arc::new(RwLock::new(quad)),
            reliability_baseline,
            reflection_budget: Mutex::new(TokenBucket::new(system2_budget, system2_refill_rate)),
            key_index: Arc::new(RwLock::new(key_index)),
            confidence_history: Arc::new(RwLock::new(ConfidenceHistory::new())),
            system2_activations: Arc::new(AtomicU64::new(0)),
            system2_throttled: Arc::new(AtomicU64::new(0)),
            contract_engine: Arc::new(tokio::sync::RwLock::new(ContractEngine::new())),
            governance_tracker: GovernanceTracker::new(),
            #[cfg(feature = "system2")]
            llm_client: None,
            default_reflection_threshold: DEFAULT_REFLECTION_THRESHOLD,
            embedder: None,
            prospective_client: None,
        }
    }

    /// Override the default System 2 activation threshold τ for all new beliefs.
    ///
    /// Beliefs created after this call inherit the new default. Beliefs already
    /// in the quad retain their per-node τ unless updated explicitly.
    pub fn with_default_tau(mut self, tau: f32) -> Self {
        self.default_reflection_threshold = tau.clamp(0.0, 1.0);
        self
    }

    /// Attach an `LlmClient` for System 2 reflection calls.
    ///
    /// Without a client, `update_belief()` returns `System1Only` even when the
    /// divergence threshold is exceeded (no budget is consumed).
    #[cfg(feature = "system2")]
    pub fn with_llm_client(mut self, client: Arc<dyn LlmClient>) -> Self {
        self.llm_client = Some(client);
        self
    }

    // ── Phase 3: contract + sovereignty builders ──────────────────────────────

    /// Attach a `BehavioralContract` to this runtime.
    ///
    /// Multiple contracts can be composed; each is evaluated on every
    /// `update_belief()` call in the order they were attached.
    pub fn with_contract(self, contract: BehavioralContract) -> Self {
        // Use try_write — safe here because we're in the builder phase (no other
        // tasks hold a reference yet).
        self.contract_engine.try_write().unwrap().contracts.push(contract);
        self
    }

    /// Attach a `MnemonicSovereignty` governance layer to this runtime.
    ///
    /// Enforces all nine primitives (auth, retention, audit, forget, cross-agent,
    /// rollback authorization, recovery verification) on every mutation.
    pub fn with_sovereignty(self, sov: MnemonicSovereignty) -> Self {
        self.contract_engine.try_write().unwrap().sovereignty = Some(sov);
        self
    }

    // ── Phase 4: prospective indexing builders ────────────────────────────────

    /// Attach a text embedder for semantic retrieval.
    ///
    /// Without an embedder, `retrieve_for_query()` uses `prospective_sim = 0.0`
    /// for all beliefs and ranks purely on uncertainty + causal centrality.
    pub fn with_embedder(mut self, embedder: Arc<dyn Embedder>) -> Self {
        self.embedder = Some(embedder);
        self
    }

    /// Attach an LLM client for write-time prospective scenario generation.
    ///
    /// When set, beliefs inserted with `prospect = true` trigger a background
    /// LLM call to generate future query scenarios (Kumiho arXiv:2603.17244).
    /// Errors are best-effort — a failed indexing call never blocks insertion.
    pub fn with_prospective_client(mut self, client: Arc<dyn ProspectiveClient>) -> Self {
        self.prospective_client = Some(client);
        self
    }

    // ── Belief mutation ───────────────────────────────────────────────────────

    /// Insert a new belief and register it in the key index.
    ///
    /// If `node.prospect == true` and a [`ProspectiveClient`] + [`Embedder`]
    /// are attached, triggers write-time scenario indexing (TD-001, Phase 4).
    /// Indexing is best-effort: failures are logged but do not block insertion.
    ///
    /// # Example
    ///
    /// ```
    /// use epica_core::{BeliefNode, BeliefQuad, BeliefValue, Provenance};
    /// use epica_runtime::BeliefRuntime;
    ///
    /// # async fn run() {
    /// let rt = BeliefRuntime::new(BeliefQuad::new(), 0.5, 10, 1.0);
    /// let id = rt.insert_belief(BeliefNode::new(
    ///     "intent",
    ///     BeliefValue::Asserted("ship".into()),
    ///     Provenance::UserStatement { turn: 0 },
    ///     0.8,
    /// )).await;
    /// assert_eq!(rt.get_by_key("intent").await, Some(id));
    /// # }
    /// # tokio::runtime::Runtime::new().unwrap().block_on(run());
    /// ```
    pub async fn insert_belief(&self, node: BeliefNode) -> BeliefId {
        let should_index = node.prospect;
        let key = node.key.clone();
        let value_repr = format!("{:?}", node.value);
        let id = {
            let mut quad = self.quad.write().await;
            quad.insert(node)
        };
        self.key_index.write().await.insert(key, id);
        if should_index {
            self.index_belief_prospective(id, &value_repr).await;
        }
        id
    }

    /// Write-time prospective indexing (TD-001, Phase 4).
    ///
    /// Called by `insert_belief()` when `node.prospect == true`.  Generates
    /// future query scenarios via the `ProspectiveClient`, embeds each scenario
    /// with the `Embedder`, and stores the result in the `ProspectiveIndex`.
    ///
    /// Best-effort: all errors are logged with `tracing::warn!` and the method
    /// returns silently — a failed indexing call never blocks insertion.
    async fn index_belief_prospective(&self, id: BeliefId, value_repr: &str) {
        let (Some(client), Some(embedder)) = (&self.prospective_client, &self.embedder) else {
            return;
        };

        // Acquire belief key under a short read lock — drop before any await.
        let belief_key = {
            let quad = self.quad.read().await;
            quad.get(id).map(|n| n.key.clone()).unwrap_or_default()
        };

        let raw = match client.generate_scenarios(&belief_key, value_repr).await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("prospective scenario generation failed for {belief_key}: {e}");
                return;
            }
        };

        let mut entries = Vec::with_capacity(raw.len());
        for scenario in raw {
            let embedding = match embedder.embed(&scenario.scenario).await {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!("embedding failed for scenario '{}': {e}", scenario.scenario);
                    vec![]
                }
            };
            entries.push(epica_core::ProspectiveEntry {
                scenario: scenario.scenario,
                embedding,
                causal_events: scenario.causal_events,
            });
        }

        let entry_count = entries.len();
        {
            let mut quad = self.quad.write().await;
            quad.prospective_index_mut().insert(id, entries);
        }
        tracing::debug!("prospective index: stored {entry_count} scenarios for {belief_key}");
    }

    /// Look up a `BeliefId` by its string key — O(1).
    pub async fn get_by_key(&self, key: &str) -> Option<BeliefId> {
        self.key_index.read().await.get(key).copied()
    }

    /// Update a belief, run System 1, and conditionally activate System 2.
    ///
    /// ## Execution flow
    ///
    /// 1. AGM revision + System 1 propagation (single write lock).
    /// 2. If contradiction: retroactively mark the previous confidence record
    ///    as incorrect.
    /// 3. System 2 check — only when an `LlmClient` is attached:
    ///    - `|fast_conf − reliability_baseline| ≤ per_node_τ` → skip.
    ///    - Otherwise → return `System2Pending { signal }` so the caller can
    ///      drive the LLM call asynchronously.
    /// 4. Append effective confidence (System 2 revised, or fast) to history.
    ///
    /// # Example
    ///
    /// ```
    /// use epica_core::{BeliefNode, BeliefQuad, BeliefValue, Provenance};
    /// use epica_runtime::{BeliefRuntime, RuntimeUpdateResult};
    ///
    /// # async fn run() {
    /// let rt = BeliefRuntime::new(BeliefQuad::new(), 0.5, 10, 1.0);
    /// let id = rt.insert_belief(BeliefNode::new(
    ///     "intent",
    ///     BeliefValue::Asserted("v0".into()),
    ///     Provenance::UserStatement { turn: 0 },
    ///     0.5,
    /// )).await;
    ///
    /// // Without an LLM client attached, even high divergence stays System 1.
    /// let result = rt.update_belief(
    ///     id,
    ///     BeliefValue::Asserted("v1".into()),
    ///     Provenance::UserStatement { turn: 1 },
    ///     0.95, // divergence 0.45 > tau 0.15 — but no client
    /// ).await.unwrap();
    /// assert!(matches!(result, RuntimeUpdateResult::System1Only));
    /// # }
    /// # tokio::runtime::Runtime::new().unwrap().block_on(run());
    /// ```
    pub async fn update_belief(
        &self,
        id: BeliefId,
        value: BeliefValue,
        provenance: Provenance,
        confidence: f32,
    ) -> Result<RuntimeUpdateResult, RuntimeError> {
        // ── Phase 3: pre-update (auth + preconditions) ────────────────────────
        let belief_key = {
            let quad = self.quad.read().await;
            quad.get(id).map(|n| n.key.clone()).unwrap_or_default()
        };
        {
            let quad = self.quad.read().await;
            let engine = self.contract_engine.read().await;
            engine.pre_update(&quad, &belief_key, "system")?;
        }

        // ── Phase 3: governance token tracking ────────────────────────────────
        {
            let engine = self.contract_engine.read().await;
            let token_limit = engine
                .contracts
                .first()
                .and_then(|c| c.governance.max_tokens_per_session);
            self.governance_tracker.track_token_usage(token_limit)?;
        }

        // ── 1. Checkpoint before revision (for recovery target) ───────────────
        let pre_revision_checkpoint = {
            let mut quad = self.quad.write().await;
            Some(quad.checkpoint())
        };

        // ── 2. Revise + System 1 (single write lock) ──────────────────────────
        let (fast_conf, was_contradiction) = {
            let mut quad = self.quad.write().await;
            let record = quad.revise(id, value, provenance, confidence)
                .map_err(EpicaError::from)?;
            let contradicted = !record.contracted.is_empty();
            quad.propagate_system1(id);
            let fast = quad.get(id).map(|n| n.fast_confidence).unwrap_or(confidence);
            (fast, contradicted)
        };

        // ── Phase 3: post-update (invariant check + recovery) ─────────────────
        {
            let engine = self.contract_engine.read().await;
            let mut quad = self.quad.write().await;
            engine.post_update(&mut quad, pre_revision_checkpoint, &belief_key, "system")?;
        }

        // ── 2. Mark previous prediction as wrong on contradiction ─────────────
        if was_contradiction {
            self.confidence_history.write().await.mark_correct(id, false);
        }

        // ── 3. System 2 activation (only when client attached) ────────────────
        let per_node_tau = self.quad.read().await
            .get(id)
            .map(|n| n.reflection_threshold)
            .unwrap_or(self.default_reflection_threshold);

        #[cfg(feature = "system2")]
        if should_activate_system2(fast_conf, self.reliability_baseline, per_node_tau) {
            if self.llm_client.is_some() {
                let belief_key = self.quad.read().await
                    .get(id)
                    .map(|n| n.key.clone())
                    .unwrap_or_default();

                let signal = DiagnosticSignal {
                    belief_key,
                    fast_confidence: fast_conf,
                    reliability_baseline: self.reliability_baseline,
                    divergence: (fast_conf - self.reliability_baseline).abs(),
                };

                // Return System2Pending — the caller spawns the LLM task.
                // Budget is NOT consumed here; the caller must do so before spawning
                // and refund via release_system2_budget() on LLM failure.
                self.confidence_history.write().await.push(id, fast_conf);
                return Ok(RuntimeUpdateResult::System2Pending { signal });
            }
        }

        // ── 4. Append fast confidence (System 1 only path) ───────────────────
        self.confidence_history.write().await.push(id, fast_conf);
        Ok(RuntimeUpdateResult::System1Only)
    }

    /// Write the System 2 revised confidence back to a belief node and record it.
    ///
    /// Called from the MCP handler's spawned async task after the LLM responds.
    /// Increments `system2_activations` and **replaces** (not appends) the fast
    /// confidence entry that `update_belief()` pushed when returning `System2Pending`.
    /// This keeps each belief revision as a single T-ECE data point: the LLM's
    /// recalibrated estimate, not a duplicate fast+slow pair.
    ///
    /// # Example
    ///
    /// ```
    /// use epica_core::{BeliefNode, BeliefQuad, BeliefValue, Provenance};
    /// use epica_runtime::BeliefRuntime;
    ///
    /// # async fn run() {
    /// let rt = BeliefRuntime::new(BeliefQuad::new(), 0.5, 10, 1.0);
    /// let id = rt.insert_belief(BeliefNode::new(
    ///     "k", BeliefValue::Asserted("v".into()),
    ///     Provenance::UserStatement { turn: 0 }, 0.9,
    /// )).await;
    ///
    /// // Simulate the handler flow: the LLM came back with 0.77.
    /// rt.apply_system2_result(id, 0.77).await;
    /// let quad = rt.read_quad().await;
    /// assert_eq!(quad.get(id).unwrap().slow_confidence, Some(0.77));
    /// # }
    /// # tokio::runtime::Runtime::new().unwrap().block_on(run());
    /// ```
    #[cfg(feature = "system2")]
    pub async fn apply_system2_result(&self, id: BeliefId, revised_confidence: f32) {
        {
            let mut quad = self.quad.write().await;
            if let Some(node) = quad.get_mut(id) {
                node.slow_confidence = Some(revised_confidence);
            }
        }
        self.system2_activations.fetch_add(1, Ordering::Relaxed);
        self.confidence_history.write().await.replace_last_confidence(id, revised_confidence);
    }

    /// Try to consume one System 2 budget token. Returns `false` if exhausted.
    pub async fn try_consume_system2_budget(&self) -> bool {
        self.reflection_budget.lock().await.try_consume(1)
    }

    /// Refund one System 2 budget token (called when the LLM call fails).
    pub async fn release_system2_budget(&self) {
        self.reflection_budget.lock().await.release(1);
    }

    /// Record a System 2 throttle event (budget exhausted, LLM call skipped).
    ///
    /// Call this from the MCP handler when `try_consume_system2_budget()` returns
    /// `false` after receiving `System2Pending` from `update_belief()`.
    pub async fn record_system2_throttle(&self) {
        self.system2_throttled.fetch_add(1, Ordering::Relaxed);
    }

    /// Access the LLM client for use in spawned async tasks.
    #[cfg(feature = "system2")]
    pub fn llm_client_arc(&self) -> Option<Arc<dyn LlmClient>> {
        self.llm_client.clone()
    }

    // ── Session metrics ───────────────────────────────────────────────────────

    /// Compute the Trajectory-ECE for this session.
    ///
    /// Returns `None` if no beliefs have a known outcome yet; call
    /// `finalize_session()` first to include surviving beliefs.
    pub async fn compute_tece(&self) -> Option<f32> {
        let history = self.confidence_history.read().await;
        let pairs = history.as_tece_input();
        epica_core::diff::tece::compute(&pairs)
    }

    /// Mark all surviving beliefs as correct and seal the session.
    ///
    /// After this call, `compute_tece()` / `session_report()` include all
    /// revisions — both contradicted (incorrect) and surviving (correct).
    pub async fn finalize_session(&self) {
        self.confidence_history.write().await.finalize_session();
    }

    /// Generate a `SessionReport` summarising this session.
    ///
    /// Call `finalize_session()` first for a complete T-ECE.
    pub async fn session_report(&self) -> SessionReport {
        let history = self.confidence_history.read().await;
        let total_revisions = history.len();
        let tece_pairs = history.as_tece_input();
        let contradictions_detected = tece_pairs.iter().filter(|(_, c)| !c).count();
        let tece = epica_core::diff::tece::compute(&tece_pairs);
        let system2_activations = self.system2_activations.load(Ordering::Relaxed);
        let system2_throttled = self.system2_throttled.load(Ordering::Relaxed);
        let calibration_target_met = tece.map(|t| t < TECE_TARGET).unwrap_or(true);

        let engine = self.contract_engine.read().await;
        let (soft, hard, critical, recovery) = engine.violation_counts();
        let drift_bounds = engine.drift_bounds();
        let governance_tokens_used = self.governance_tracker.tokens_used();

        SessionReport {
            tece,
            total_revisions,
            contradictions_detected,
            system2_activations,
            system2_throttled,
            calibration_target_met,
            soft_violations: soft,
            hard_violations: hard,
            critical_violations: critical,
            recovery_actions_taken: recovery,
            drift_bounds,
            governance_tokens_used,
        }
    }

    // ── Retrieval ─────────────────────────────────────────────────────────────

    /// Retrieve beliefs most relevant to `query` within a token budget.
    ///
    /// Multicriteria score: prospective_sim×0.45 + uncertainty_bonus×0.25
    /// + causal_centrality×0.20 − decay_penalty×0.10.
    ///
    /// Phase 4: `prospective_sim` is non-zero when an [`Embedder`] is attached
    /// and beliefs have been indexed via `prospect = true` (TD-001 resolved).
    /// Budget approximation: 100 tokens per belief.
    pub async fn retrieve_for_query(
        &self,
        query: &str,
        budget_tokens: usize,
    ) -> Vec<BeliefSnapshot> {
        // Embed the query BEFORE acquiring the quad read lock.
        // Awaiting inside a held RwLockReadGuard deadlocks under write pressure.
        let query_emb: Vec<f32> = match &self.embedder {
            Some(e) => e.embed(query).await.unwrap_or_default(),
            None => vec![],
        };

        let now_ms = now_ms();
        let quad = self.quad.read().await;
        let max_count = (budget_tokens / 100).max(1);

        let mut scored: Vec<(BeliefId, f32)> = quad
            .iter()
            .filter(|(_, n)| !n.is_expired(now_ms))
            .map(|(id, node)| {
                let prospective_sim = if query_emb.is_empty() {
                    0.0
                } else {
                    quad.prospective_index().max_prospective_sim(id, &query_emb)
                };
                let causal_centrality = quad.causal().centrality(id);
                let decay = quad.decay_factor(id, now_ms);
                let score = compute_score(prospective_sim, node.fast_confidence, causal_centrality, decay);
                (id, score)
            })
            .collect();

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let version = quad.version();
        scored
            .into_iter()
            .take(max_count)
            .filter_map(|(id, _)| {
                quad.get(id).map(|n| BeliefSnapshot {
                    id,
                    key: n.key.clone(),
                    value: n.value.clone(),
                    fast_confidence: n.fast_confidence,
                    slow_confidence: n.slow_confidence,
                    version_at_snapshot: version,
                })
            })
            .collect()
    }

    // ── Checkpoint / rollback ─────────────────────────────────────────────────

    /// Create a checkpoint of the current quad state.
    pub async fn checkpoint(&self) -> epica_core::CheckpointId {
        self.quad.write().await.checkpoint()
    }

    /// Roll back to a previously captured checkpoint.
    pub async fn rollback_to(
        &self,
        id: epica_core::CheckpointId,
    ) -> Result<epica_core::BeliefQuadDiff, epica_core::RollbackError> {
        self.quad.write().await.rollback_to(id)
    }

    // ── Read-only access ──────────────────────────────────────────────────────

    /// Read-only access to the underlying quad.
    pub async fn read_quad(&self) -> tokio::sync::RwLockReadGuard<'_, BeliefQuad> {
        self.quad.read().await
    }

    /// Reference to the per-session confidence history.
    ///
    /// Exposed for external oracle use (e.g. eval harnesses that have
    /// ground-truth labels).
    pub fn confidence_history(&self) -> &Arc<RwLock<ConfidenceHistory>> {
        &self.confidence_history
    }

    /// Current System 2 activation count (monotonic, `Ordering::Relaxed`).
    pub fn system2_activations(&self) -> u64 {
        self.system2_activations.load(Ordering::Relaxed)
    }

    /// Current System 2 throttle count (monotonic, `Ordering::Relaxed`).
    pub fn system2_throttled(&self) -> u64 {
        self.system2_throttled.load(Ordering::Relaxed)
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use epica_core::{BeliefNode, BeliefValue, Provenance};

    fn make_runtime() -> BeliefRuntime {
        BeliefRuntime::new(BeliefQuad::new(), 0.5, 10, 1.0)
    }

    #[test]
    fn new_runtime_is_valid() {
        let rt = make_runtime();
        assert!(rt.confidence_history.try_read().is_ok());
        assert_eq!(rt.system2_activations(), 0);
        assert_eq!(rt.system2_throttled(), 0);
    }

    #[tokio::test]
    async fn insert_belief_registers_key_index() {
        let rt = make_runtime();
        let node = BeliefNode::new(
            "test_key",
            BeliefValue::Asserted("hello".into()),
            Provenance::UserStatement { turn: 0 },
            0.9,
        );
        let id = rt.insert_belief(node).await;
        let looked_up = rt.get_by_key("test_key").await;
        assert_eq!(looked_up, Some(id));
    }

    #[tokio::test]
    async fn retrieve_returns_beliefs_sorted_by_score() {
        let rt = make_runtime();
        for i in 0..5u32 {
            let node = BeliefNode::new(
                format!("key_{i}"),
                BeliefValue::Asserted(format!("val_{i}")),
                Provenance::UserStatement { turn: i },
                0.5 + i as f32 * 0.1,
            );
            rt.insert_belief(node).await;
        }
        let results = rt.retrieve_for_query("", 10_000).await;
        assert!(!results.is_empty());
        let monotone = results.windows(2).all(|w| {
            compute_score(0.0, w[0].fast_confidence, 0.0, 1.0)
                >= compute_score(0.0, w[1].fast_confidence, 0.0, 1.0)
        });
        assert!(monotone || results.len() < 2);
    }

    #[tokio::test]
    async fn session_report_empty_has_no_tece() {
        let rt = make_runtime();
        let report = rt.session_report().await;
        assert!(report.tece.is_none());
        assert_eq!(report.total_revisions, 0);
        assert!(report.calibration_target_met);
    }
}
