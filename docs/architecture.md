# Architecture

## What this document covers

Why Epica uses four orthogonal graphs instead of one, the invariants
that hold across all graph operations, the data flow through the
runtime, the design tradeoffs made explicitly, and the failure modes
this architecture prevents.

Post-public-review hardening (Sprints 1–4) added three orthogonal
subsystems — semantic-equivalence checking, continuous Bayesian-surprise
monitoring, and a tamper-evident audit ledger — without changing the
core invariants below.

---

## System map (12 crates)

```text
┌─────────────────────────────────────────────────────────────────────────┐
│  Application / agent (Anthropic SDK, OpenAI, MCP host, LangGraph, …)    │
└─────────────────────────────────────────────────────────────────────────┘
                │ Rust API  │ Python SDK  │ MCP HTTP  │ epica-verify CLI
                ▼            ▼             ▼            ▼
┌──────────────────┐  ┌──────────────┐  ┌───────────┐  ┌──────────────┐
│  epica-core      │  │ epica-python │  │ epica-mcp │  │ epica-zk-    │
│  BeliefQuad      │  │ PyO3 SDK     │  │ Axum 2026 │  │ evidence     │
│  AGM K*2–K*6     │  └──────────────┘  │ 16 routes │  │ EvidenceR.   │
│  System 1        │                    └───────────┘  │ Ed25519      │
│  Embedding trait │                                   └──────────────┘
└──────────────────┘
       │
       ├─ epica-runtime ─── dual-process (System 1 + System 2), retrieval,
       │                     T-ECE history, ContractEngine, FEP hook
       │
       ├─ epica-contracts ── BehavioralContract C=(P,I,G,R),
       │                     9 Mnemonic Sovereignty primitives,
       │                     AuditLedger (BLAKE3 Merkle chain)
       │
       ├─ epica-active-     ActiveInferenceMonitor (Friston VFE)
       │  inference         opt-in via feature; observes each insert
       │
       ├─ epica-anthropic ── AnthropicLlmClient + ProspectiveClient
       │
       ├─ epica-openai ──── OpenAiLlmClient + OpenAiEmbeddingProvider
       │                     (OpenAI-compatible: Voyage, Together, TEI)
       │
       ├─ epica-memory ──── LongTermMemoryStore + Redis + Neo4j (neo4rs)
       │
       ├─ epica-macros ──── #[derive(BeliefState)]
       │
       └─ epica-benchmarks  Synthetic ALFWorld/WebShop traces;
                            epica-bench CLI; CSV + Markdown reporters
```

The default `cargo check --workspace --exclude epica-python` compiles
every crate above with its feature defaults. Opt-in features
(`neo4j`, `active-inference`, `risc0`, `sled-store`) are individually
verifiable from CI.

---

## Why four graphs?

MAGMA (arXiv:2601.03236) runs ablations on a monolithic belief graph
vs. four orthogonal graphs and demonstrates that:

1. **Retrieval ambiguity**: a single graph cannot distinguish
   "A happened before B" (temporal) from "A causes B" (causal) — both
   are edges. Retrieval must guess which semantic to apply.
2. **Traversal interference**: causal DFS visits semantically unrelated
   nodes that happen to share temporal edges.
3. **Interpretability collapse**: the meaning of a path through a
   monolithic graph is undefined without edge-type inspection at every
   step.

Epica's `BeliefQuad` maintains four
`petgraph::stable_graph::StableDiGraph<BeliefId, EdgeType>` instances
synchronised over a single `SlotMap<BeliefId, BeliefNode>`.

Each graph has exactly one semantics:

- `SemanticGraph` — subsumes / contradicts / synonymous.
- `TemporalGraph` — before / after, with temporal decay.
- `CausalGraph` — causes / is caused by, with effect size and Noisy-OR.
- `EntityGraph` — entity membership / role assignment.

---

## Invariants

### 1. `BeliefId` is the single source of truth

The four graphs store `BeliefId` as node weights — NOT copies of
`BeliefNode`. Modifying a node's value only touches the `SlotMap`;
graph traversal returns `BeliefId`s for O(1) `SlotMap` lookup.
Removing a node from the `SlotMap` does not break graph indices —
lookup into the (now-empty) `SlotMap` returns `None`.

### 2. O(1) graph node lookup

Each graph maintains a `HashMap<BeliefId, petgraph::stable_graph::NodeIndex>`.
All graph methods go through `self.indices.get(&id)` and return early
when the id is not registered. Failing to maintain this map is the
most common petgraph bug class; the unit tests assert the invariant
under every public operation.

### 3. `StableDiGraph` for safe removal

`petgraph::stable_graph::StableDiGraph` does NOT invalidate
`NodeIndex` values when a node is removed. Critical because AGM
contraction (`apply_contraction()`) must not break the `NodeIndex`
values stored in `indices` for other beliefs.

### 4. System 1 cycle guard

`CausalGraph` is user-constructed and CAN contain cycles. The Noisy-OR
propagation guards with `HashSet<BeliefId>`:

```rust
fn propagate_system1_inner(&mut self, id: BeliefId, visited: &mut HashSet<BeliefId>) {
    if !visited.insert(id) { return; } // already visited — cycle guard
    // ...
}
```

Non-optional. A cycle without it would cause infinite recursion.

### 5. `PostulateAudit` is an audit record + a critical gate for K\*2/3/5/6

`PostulateAudit::verify()` captures the pre-revision quad state and
checks all six postulates **before** the mutation. K\*2 (success),
K\*3 (inclusion), K\*5 (consistency) and K\*6 (extensionality) are
critical — a violation rejects the revision with
`BeliefRevisionError::PostulateViolation { postulate }`. K\*4
(vacuity) is informational only: `vacuity = false` means contraction
is needed, which is a legitimate outcome.

K\*6 is the real semantic postulate, not a structural stub
([details below](#k6-semantic-equivalence)).

### 6. `ProspectiveIndex` exists from Phase 1

`ProspectiveIndex` is present in `BeliefQuad` from Phase 1 so the
serialisation format stays stable. `index_belief()` is now wired to
real LLM embeddings via the `ProspectiveClient` trait (resolved
TD-001 in Phase 4).

### 7. The audit ledger is opt-in but tamper-evident when enabled

`AuditPolicy::with_ledger()` attaches an `Arc<Mutex<AuditLedger>>`
([code](../crates/epica-contracts/src/sovereignty/ledger.rs)). Every
`emit()` then seals the entry into a BLAKE3 hash chain *before*
dispatching to the destination — so a destination write failure
never breaks the chain. The ledger exposes `verify_chain()`,
`merkle_root()`, and `merkle_proof(seq)`; `epica-verify` consumes
all three.

---

## Data flow

```text
user/tool ─► BeliefNode ─► BeliefQuad.insert()
                          │
                          ▼
                  [4 graphs add_node()]
                          │
                          ▼
           BeliefQuad.revise() ─── PostulateAudit.verify()
                  │                 │
            ┌─────┴─────┐           ▼
            ▼           ▼      K*2/3/5/6 check (HARD)
   expand_only()  contract+expand
            └─────┬─────┘
                  ▼
            fast_confidence updated
                  │
                  ▼
       BeliefQuad.propagate_system1()      ← cycle-guarded Noisy-OR
                  │
                  ▼
    BeliefRuntime.update_belief()
       │ if |fast_conf − baseline| > τ AND budget > 0:
       │     System2Pending → spawn LlmClient::reflect()
       │
       ▼
    BehavioralContract.check_invariants()
                  │
                  ▼
    ConfidenceHistory.push(id, effective_conf)
                  │
       ┌──────────┴──────────┐
       ▼                     ▼
  ActiveInferenceMonitor    AuditPolicy.emit()
   .observe(quad, node)    │
   (when feature on)       ▼
   → SurpriseSignal       AuditLedger.append()
                          ├─► self_hash = BLAKE3(prev || entry)
                          └─► (optional) sealed into EvidenceReceipt
                              by epica-zk-evidence
```

---

## Key subsystems added post-public-review

### K\*6 semantic equivalence

[`epica-core/src/embedding/`](../crates/epica-core/src/embedding/mod.rs)
defines the `EmbeddingProvider` trait — sync on the hot path, async
warm-up off-path. The default `NullEmbeddingProvider` reproduces the
pre-Sprint-1 behaviour exactly; `CachedEmbeddingProvider<B>` wraps any
backend with an in-memory cache;
[`OpenAiEmbeddingProvider`](../crates/epica-openai/src/embeddings.rs)
talks the OpenAI-compatible embeddings API (works against OpenAI,
Voyage, Together, self-hosted TEI servers).

When a provider is installed via
`BeliefQuad::set_embedding_provider(Arc<dyn EmbeddingProvider>)`,
`value_contradicts_semantic` consults the cache for `Asserted`/`Asserted`
pairs and classifies by cosine similarity into `Equivalent` /
`Contradicts` / `Undecided` bands (defaults: ≥ 0.92 / ≤ −0.30). The
verdict trace surfaces in `PostulateAudit::verdict_trace` and the
in-quad paraphrase witness in `extensionality_witness` — K\*6
violations are diagnosable, not silent.

When no embedding is cached, the comparison falls back to the literal
behaviour. Zero-regression promise.

### Active Inference monitor (opt-in)

[`epica-active-inference`](../crates/epica-active-inference/) ships a
variational free-energy monitor (Friston / pymdp / RxInfer lineage).
The Sprint-2 mapping treats:

- hidden state `s_i` = truth value of belief `i`,
- posterior `q_i(s = true)` = `fast_confidence`,
- prior `p_i(s = true)` = NoisyOr of causal-graph parents (or `0.5`
  uninformative when none),
- `F = Σ_i KL(q_i ‖ p_i) − E_q[ln p(o_last ‖ s_last)]`.

The monitor lives behind the `active-inference` feature on `epica-runtime`.
With the feature on and a monitor attached, every `insert_belief` calls
`observe(&quad, &node)` and emits a `SurpriseSignal`. The hook never
blocks the mutation — failures (mutex poison, race) are logged.

### Audit ledger + EvidenceReceipt

[`epica-contracts/src/sovereignty/ledger.rs`](../crates/epica-contracts/src/sovereignty/ledger.rs)
adds a Merkle-evident chain over `AuditEntry`s. Each appended entry
records `self_hash = BLAKE3(prev_hash || canonical_json(entry))`.
`verify_chain()` checks every invariant in O(N); `merkle_root()`
emits a 32-byte commitment in O(N); `merkle_proof(seq)` returns an
O(log N) inclusion proof.

[`epica-zk-evidence`](../crates/epica-zk-evidence/) wraps the ledger
with Ed25519 signatures over a domain-separated binding:
`BLAKE3("epica-evidence-receipt-v1\0" || root || start || end ||
ledger_len || pubkey || schema_version)`. The receipt is JSON,
hex-encoded for all binary fields, schema-versioned. The
[`epica-verify`](../crates/epica-zk-evidence/src/bin/verify.rs) CLI
exposes `keygen` / `seal` / `verify`.

A `risc0` feature gate reserves room for a future zkVM backend that
additionally proves "this Merkle root corresponds to N valid AGM
transitions" — see [`zk_skeleton.rs`](../crates/epica-zk-evidence/src/zk_skeleton.rs)
for the documented landing surface.

---

## Design tradeoffs

### External causal graph vs. internal model uncertainty

Epica propagates confidence through an external, agent-maintained
causal graph (Noisy-OR). The paper's UAM module propagates through
Transformer attention weights. Epica's approach is auditable and
deterministic; it does not track model-internal uncertainty.

**Tradeoff accepted**: interpretability + determinism over fidelity to
the paper's uncertainty model.

### `SlotMap` for storage, `petgraph` for structure

The `BeliefId → NodeIndex` `HashMap` indirection adds one hashmap
lookup per graph operation in exchange for safe removal and stable
indices under `StableDiGraph`.

**Tradeoff accepted**: one extra lookup for stable indices.

### Async System 2 via spawn

System 2 was sync in Phase 2; Sprint-1 hardening moved it to async via
`System2Pending` + a spawned task. The token-bucket budget is refunded
on transient LLM failure. Caller-side observability is via the
`task_id` returned from `update_belief()`.

**Tradeoff accepted**: complexity (background task + lock discipline)
for non-blocking belief updates.

### K\*6 sync via cache, real semantics via embeddings

The K\*6 hot path stays sync (`embed_cached` returns `Option<Vec<f32>>`
from an in-memory cache). Real embedding computation is async, but
runs off-path in `warm_async`. Cache miss → literal fallback →
identical to Sprint-0 behaviour.

**Tradeoff accepted**: a "warm-up before mutation" discipline in
exchange for a sync `revise()` API.

### Free-energy monitor is opt-in, never blocking

`ActiveInferenceMonitor::observe()` is called *after* the mutation
completes. A budget breach emits `tracing::warn!` and a `SurpriseSignal`
— it does **not** roll back the insert. Hard enforcement is a
contract invariant's job.

**Tradeoff accepted**: telemetry-first FEP audit over hard halt.

### Ledger sealing in the hot path, ZK proof generation off-path

Every `emit()` sealing is sync (a single BLAKE3 hash) — well inside
the runtime's 50K ops/s budget. The Sprint-3.2 `EvidenceReceipt`
sealing is also sync (single Ed25519 sign), but typically done at
session boundaries, not per-insert. A future RISC Zero proof
generation is explicitly off-path and batched.

**Tradeoff accepted**: cryptographic linkage in the hot path
(microsecond cost) for offline verifiability.

---

## Failure modes this architecture prevents

| Failure mode | Prevention mechanism |
|---|---|
| Agent contradicts itself silently | AGM `check_contradiction()` + K\*4 vacuity guard |
| **Agent silently revises a paraphrase as if new** | K\*6 semantic equivalence via `EmbeddingProvider` |
| Confidence propagates through cycles | `propagate_system1()` cycle guard |
| Graph indices invalidated after removal | `StableDiGraph` stable `NodeIndex` |
| Checkpoint format breaks on new fields | `#[serde(default)]` + `ProspectiveIndex` from Phase 1 |
| System 2 budget leak in test environments | `TokenBucket` only consumes when `LlmClient` is present |
| Contract violations propagate silently | `BehavioralContract.check_invariants()` after every update |
| Belief erasure unconfirmed | `ForgetPolicy.verify_fn` traverses all four graphs |
| **Whole-agent drift escapes contract invariants** | `ActiveInferenceMonitor` reports `exceeds_budget` |
| **Audit trail edited post-hoc** | `AuditLedger::verify_chain()` detects any single-entry tamper |
| **Producer signs receipts they later deny** | Ed25519 signature in `EvidenceReceipt` (non-repudiation) |

---

## What this architecture does not solve

- **Semantic equivalence in `Inferred`/`Deterministic` JSON values**:
  the K\*6 embedding path applies to `Asserted/Asserted` only.
  Structural-equality fallback still applies elsewhere — documented as
  TD-P8-003.
- **Distributed belief consistency**: concurrent writes are serialised
  by the `RwLock` within a single process; cross-process consistency is
  not provided.
- **Model-internal uncertainty**: Epica does not access LLM attention
  weights or logits. Confidence is agent-maintained.
- **Formal proof of AGM compliance**: tested against postulate
  definitions with proptest (256+ cases per postulate), not formally
  proven in a proof assistant.
- **Long-term causal structure learning**: the causal graph is
  populated by the agent or user, not learned from data.
- **Zero-Knowledge proofs over AGM validity**: ledger commitment + ed25519
  shipped today; STARK proof of AGM transitions deferred (see
  `zk_skeleton.rs`).

---

## Implementation pointers

| Component | Location |
|---|---|
| `BeliefQuad` | [`crates/epica-core/src/quad/mod.rs`](../crates/epica-core/src/quad/mod.rs) |
| Four graphs | [`crates/epica-core/src/quad/{semantic,temporal,causal,entity}.rs`](../crates/epica-core/src/quad/) |
| AGM revision | [`crates/epica-core/src/revision/agm.rs`](../crates/epica-core/src/revision/agm.rs) |
| `PostulateAudit` (K\*1–K\*6 real) | [`crates/epica-core/src/revision/postulates.rs`](../crates/epica-core/src/revision/postulates.rs) |
| `EmbeddingProvider` | [`crates/epica-core/src/embedding/mod.rs`](../crates/epica-core/src/embedding/mod.rs) |
| `OpenAiEmbeddingProvider` | [`crates/epica-openai/src/embeddings.rs`](../crates/epica-openai/src/embeddings.rs) |
| System 1 Noisy-OR | [`crates/epica-core/src/system1/mod.rs`](../crates/epica-core/src/system1/mod.rs) |
| `BeliefRuntime` | [`crates/epica-runtime/src/runtime.rs`](../crates/epica-runtime/src/runtime.rs) |
| `BehavioralContract` | [`crates/epica-contracts/src/contract.rs`](../crates/epica-contracts/src/contract.rs) |
| `MnemonicSovereignty` | [`crates/epica-contracts/src/sovereignty/mod.rs`](../crates/epica-contracts/src/sovereignty/mod.rs) |
| `AuditLedger` (BLAKE3 Merkle) | [`crates/epica-contracts/src/sovereignty/ledger.rs`](../crates/epica-contracts/src/sovereignty/ledger.rs) |
| `ActiveInferenceMonitor` | [`crates/epica-active-inference/src/monitor.rs`](../crates/epica-active-inference/src/monitor.rs) |
| `EvidenceReceipt` + Ed25519 | [`crates/epica-zk-evidence/src/{receipt,prover,verifier}.rs`](../crates/epica-zk-evidence/src/) |
| `epica-verify` CLI | [`crates/epica-zk-evidence/src/bin/verify.rs`](../crates/epica-zk-evidence/src/bin/verify.rs) |
| `epica-bench` CLI | [`crates/epica-benchmarks/src/bin/bench.rs`](../crates/epica-benchmarks/src/bin/bench.rs) |
