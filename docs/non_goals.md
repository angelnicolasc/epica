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
- Cross-session persistent graph storage (Redis covers working memory; Neo4j is not available in this phase)
- Distributed graph partitioning or replication

### Not a truth oracle

Epica manages **belief state**, not truth. A belief with confidence 0.95 is not 95% likely to be factually correct - it reflects the agent's epistemic state given available evidence. Epica does not validate beliefs against external facts.

### Not a replacement for output-level guardrails

Epica operates on the agent's internal belief state, before the agent acts. It does not inspect, filter, or block LLM output strings. Output-level guardrails (AgentAssert, content filters, output parsers) operate at a different layer and are complementary, not redundant.

### Not a replica of Transformer internals

System 1 in Epica propagates confidence through an external causal graph. The paper's UAM module propagates uncertainty through attention weights. These are different mechanisms. Epica does not access model internals and does not claim to replicate internal uncertainty estimates.

### Not a general semantic equivalence detector

K\*6 compliance requires detecting when two belief values are logically equivalent. Epica currently compares `BeliefValue` structurally. Semantic equivalence (detecting that "Paris is in France" and "France contains Paris" are the same belief) requires embeddings and is not implemented in the core revision path (TD-003).

### Not formally proven to scale

Epica is designed for single-agent, single-process deployments. It has not been benchmarked or stress-tested at scale. Claims about performance at 10K nodes are targets, not measured results (Phase 1 benchmarks pending).

---

## Accepted tradeoffs

| Tradeoff | Accepted in favor of |
|---------|---------------------|
| K\*6 structural only | Synchronous `revise()` API; no async LLM on hot path |
| System 2 is synchronous | Simpler locking model; no background thread complexity |
| PostulateAudit is non-blocking in release | Resilience over hard enforcement; contracts cover hard enforcement |
| No cross-process belief consistency | Single-process simplicity; distributed consistency requires a different architecture |
| `HashEmbedder` fallback (no model dependency) | Zero external dependency for offline use; embedding quality tradeoff |
| Manual `.pyi` stubs over auto-generated | More precise type information; more maintenance burden |

---

## What would break at scale

- **In-memory `TaskStore`**: tasks lost on MCP server restart. Not suitable for multi-instance deployments without a shared persistence layer.
- **Synchronous System 2**: an agent that triggers System 2 frequently will block on LLM round trips in the `update_belief()` call. Not suitable for high-throughput belief ingestion without an async redesign.
- **Single-process `RwLock`**: `BeliefQuad` and `BeliefRuntime` are protected by `RwLock`/`Mutex`. Write-heavy workloads in multi-threaded contexts will contend on the write lock.
- **In-process causal graph**: the causal graph lives in memory. At very large belief counts (hundreds of thousands), memory pressure and traversal cost may become significant. No data on this exists.

---

## What depends on external model quality

- **System 2 recalibration**: the quality of `slow_confidence` depends on the LLM's ability to introspect and revise confidence estimates. If the model is poorly calibrated, System 2 may degrade T-ECE.
- **ProspectiveIndex with real embeddings**: when `AnthropicProspectiveClient` is used instead of `HashEmbedder`, retrieval quality depends on the embedding model's semantic understanding.
- **Semantic contradiction detection (TD-003)**: once implemented, contradiction detection via embeddings will depend on the embedding model's representation of negation and paraphrase.

---

## What is not formally proven

- AGM postulate compliance (K\*2-K\*5) is tested, not formally proven.
- K\*6 is approximated, not compliant.
- T-ECE calibration is validated on a deterministic benchmark, not on real tasks.
- Drift bound D\* = α/γ is an analytical estimate from CLT, not an empirically validated guarantee.
- ForgetPolicy erasure is verified by graph traversal, not by a cryptographic erasure proof.
