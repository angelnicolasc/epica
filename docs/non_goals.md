# Non-Goals

This document defines what Epica explicitly does not attempt. Understanding the boundaries is as important as understanding the capabilities.

---

## What Epica is not

### Not a theorem prover

Epica does not formally prove logical consequences. AGM belief revision is not theorem proving - it is a consistency-maintenance procedure. `PostulateAudit` checks postulate compliance against the implementation's own definitions, not against a formal model in a proof assistant (Coq, Lean, Isabelle, etc.).

### Not a full knowledge graph platform

Epica is an embeddable runtime library, not a knowledge graph system. It does not provide:
- SPARQL or Cypher query languages
- Schema inference or ontology management
- Cross-session persistent graph storage at scale (Redis covers working memory; Neo4j via `neo4rs 0.8` is available opt-in but without distributed partitioning or replication)
- Distributed graph partitioning or replication

### Not a truth oracle

Epica manages **belief state**, not truth. A belief with confidence 0.95 is not 95% likely to be factually correct - it reflects the agent's epistemic state given available evidence. Epica does not validate beliefs against external facts.

### Not a replacement for output-level guardrails

Epica operates on the agent's internal belief state, before the agent acts. It does not inspect, filter, or block LLM output strings. Output-level guardrails (AgentAssert, content filters, output parsers) operate at a different layer and are complementary, not redundant.

### Not a replica of Transformer internals

System 1 in Epica propagates confidence through an external causal graph. The paper's UAM module propagates uncertainty through attention weights. These are different mechanisms. Epica does not access model internals and does not claim to replicate internal uncertainty estimates.

### Not a universal semantic equivalence detector

K\*6 compliance requires detecting when two belief values are logically equivalent. Epica implements this via `EmbeddingProvider` for `Asserted/Asserted` pairs — paraphrases of the same intent are classified using cosine similarity. The semantic path does not apply to `Inferred(JsonValue)` or `Deterministic(JsonValue)` variants, which use structural equality (TD-P8-003). For those variants, literal equality is the correct test in practice.

### Not formally proven to scale

Epica is designed for single-agent, single-process deployments. It has not been benchmarked or stress-tested at scale. Criterion benchmarks cover the core hot paths (insert, revise, checkpoint/rollback) up to 10 000 nodes; end-to-end harness benchmarks cover 200 synthetic trajectories per suite. Real-world agent workloads at higher belief counts are not yet measured.

---

## Accepted tradeoffs

| Tradeoff | Accepted in favor of |
|---------|---------------------|
| K\*6 semantic path covers `Asserted/Asserted` only | Covers the dominant real-world case; JSON-structured `Inferred`/`Deterministic` variants use structural equality where it is the correct test |
| `revise()` is sync; embedding warm-up is async off-path | Non-blocking hot path; callers pre-warm the cache before mutation |
| FEP monitor is telemetry-only by default (never blocks mutation) | Hard enforcement is a `BehavioralContract` invariant's job |
| No cross-process belief consistency | Single-process simplicity; distributed consistency requires a different architecture |
| `NullEmbeddingProvider` default (no model dependency) | Zero external dependency for offline use; switching to `OpenAiEmbeddingProvider` enables full K\*6 |
| Manual `.pyi` stubs over auto-generated | More precise type information; more maintenance burden |

---

## What would break at scale

- **Unbounded embedding cache**: `CachedEmbeddingProvider` holds all embeddings in memory with no eviction; long-running agents will see unbounded memory growth (TD-P8-002).
- **In-process audit ledger**: `AuditLedger` is in-memory only; crash recovery requires a `LedgerStore` trait (TD-P9-001). Cross-process audit aggregation is not provided.
- **Single-process `RwLock`**: `BeliefQuad` and `BeliefRuntime` are protected by `RwLock`/`Mutex`. Write-heavy workloads in multi-threaded contexts will contend on the write lock.
- **In-process causal graph**: the causal graph lives in memory. At very large belief counts (hundreds of thousands), memory pressure and traversal cost may become significant. No data on this exists at scale.

---

## What depends on external model quality

- **System 2 recalibration**: the quality of `slow_confidence` depends on the LLM's ability to introspect and revise confidence estimates. If the model is poorly calibrated, System 2 may degrade T-ECE.
- **ProspectiveIndex with real embeddings**: when `AnthropicProspectiveClient` is used instead of `HashEmbedder`, retrieval quality depends on the embedding model's semantic understanding.
- **K\*6 semantic classification**: contradiction detection via cosine similarity depends on the embedding model's representation of negation and paraphrase. The classification bands (≥ 0.92 for Equivalent, ≤ −0.30 for Contradicts) are tunable defaults, not universal truths.

---

## What is not formally proven

- AGM postulate compliance (K\*2–K\*6) is tested with proptest (256+ cases each), not formally proven in a proof assistant.
- K\*6 compliance is "exact given the provider's verdict" — the implementation enforces that equivalent inputs produce identical revision outcomes, but "equivalent" is whatever the `EmbeddingProvider` classifies as `Equivalent`. The provider's classification is the test of logical equivalence at this layer.
- T-ECE calibration is validated on a deterministic benchmark and on synthetic trajectories (200 per suite). Real-world calibration on live agent workloads is not yet measured.
- Drift bound D\* = α/γ is an analytical estimate from CLT, not an empirically validated guarantee.
- ForgetPolicy erasure is verified by graph traversal, not by a cryptographic erasure proof.
- The Active Inference VFE mapping is over the `BeliefQuad`, not LLM internals. The FEP is substrate-agnostic; the monitor measures whole-agent calibration drift, not per-token uncertainty.
