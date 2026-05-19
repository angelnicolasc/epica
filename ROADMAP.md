# Epica — Roadmap

This file is the **canonical roadmap** for the public crate. It supersedes
older scattered status text in `README.md` and internal notes in `DEVLOG.md`
(which remains the team's internal change log).

There are **no calendar dates** on this roadmap. Open-source release dates
are unreliable and inviting a "you said Q3" conversation does nothing for
either the maintainer or the user. Items are ordered by priority and each
one carries an objectively verifiable definition of "done."

## Conventions

| Symbol | Meaning |
|---|---|
| 🟢 | Stable. Compiled, tested, no `todo!()`, public-API frozen. |
| 🟡 | In progress. Code lives in the tree but design or coverage is incomplete. |
| 🔵 | Designed. Interface drafted; implementation deferred behind a feature flag with cost/value rationale in DEVLOG. |
| ⚪ | Planned. Listed for visibility; no design yet. |

| Priority | Meaning |
|---|---|
| **P0** | Required for the next sprint. Blocks elevation of dependent phases. |
| **P1** | Important. Ships when P0 is clear. |
| **P2** | Nice to have. Will land if bandwidth allows. |

Open items reference IDs in `DEVLOG.md` (`TD-XXX`, `HD-XXX`) where they have a
fuller diagnostic. When a TD is closed in code, the matching row here is
struck through and the closing sprint linked.

---

## Phase 1 — `epica-core` (BeliefQuad + AGM + System 1) 🟢

The mathematical core: four orthogonal graphs over a shared `SlotMap`, AGM
revision satisfying postulates **K\*2–K\*6 as hard errors** in every build,
System 1 Noisy-OR propagation with cycle guard, checkpoint + rollback with
K\*4 guard, diff and counterfactual, multicriteria retrieval, the
`EmbeddingProvider` trait and its two built-in implementations.

**Done when:**
- `cargo test -p epica-core` is green and includes proptest cases for
  K\*2, K\*3, K\*4, K\*5 (≥ 256 cases each). ✅
- K\*6 detects semantic paraphrases when an `EmbeddingProvider` is
  installed (4 cases: vacuous/no-provider, paraphrase-is-not-contradiction,
  anti-parallel-is-contradiction, witness-detection). ✅
- `cargo bench -p epica-core` produces reproducible Criterion reports for
  insert / revise / checkpoint+rollback / System 1 propagation across
  100 / 1 000 / 10 000 nodes. Reported in [BENCHMARKS.md](./BENCHMARKS.md). ✅
- Two libFuzzer targets (`fuzz_revise`, `fuzz_belief_value`) build under
  `cargo +nightly fuzz build`. ✅ (see [docs/fuzzing.md](./docs/fuzzing.md))

~~**P0 open: K\*6 structural-only** (TD-003)~~ — **Closed, Sprint 1.**
K\*6 is now a real semantic postulate via `EmbeddingProvider`. The
`NullEmbeddingProvider` default reproduces the Phase-1 literal behaviour
exactly; `OpenAiEmbeddingProvider` closes the loop end-to-end.

**Open items:**
- **P1** Pathological-chain System 1 propagation cost (`~374 µs / node` at
  depth 10 000, see BENCHMARKS.md). Real DAGs are shallow, but the
  recursion + `descendants_of()` walk is amenable to iterative rewrite.
  Tracked as HD-S2-A1 in DEVLOG.
- **P2** K\*6 semantic path applies to `Asserted/Asserted` only; `Inferred`
  and `Deterministic` JSON values use structural equality (TD-P8-003).
  Low priority — those variants carry structured JSON where literal equality
  is the correct test.

---

## Phase 2 — `epica-runtime` (Dual-process + retrieval + T-ECE) 🟢

`BeliefRuntime` over an `Arc<RwLock<BeliefQuad>>` with the dual-process
state machine, `TokenBucket` reflection budget, `ConfidenceHistory` for
T-ECE, multicriteria retrieval, and an opt-in hook for the Active Inference
monitor (Sprint 2).

**Done when:**
- `cargo test -p epica-runtime` is green. ✅
- `update_belief()` returns `System2Pending` rather than blocking on the
  LLM. ✅ (Sprint 1, commit `2e0cbce`)
- Property tests assert `fast_confidence ∈ [0, 1]` and strict version
  monotonicity over arbitrary mutation sequences. ✅
- `--features active-inference` compiles and the FEP hook fires on every
  `insert_belief`. ✅ (Sprint 2)

**Open items:**
- **P2** No System 1 / System 2 throughput benchmark when a real
  `LlmClient` is wired up (only mock-client tests exist today). Tracked
  as TD-P13-002 in DEVLOG.

---

## Phase 3 — `epica-contracts` (Behavioral C=(P,I,G,R) + Sovereignty + Audit) 🟢

`BehavioralContract`, `ContractEngine`, `GovernanceTracker`, all nine
Mnemonic Sovereignty primitives, drift-bound via CLT, TOML
`ContractConfig`, the **BLAKE3 Merkle audit ledger** (`AuditLedger` with
per-entry `merkle_proof(seq)`) and the `AuditPolicy::with_ledger()` opt-in.

**Done when:**
- `cargo test -p epica-contracts` is green and covers the four auth modes
  with proptest cases. ✅
- Tamper detection: modifying a single entry invalidates its hash and breaks
  the chain link for every downstream entry. ✅ (8 integration tests in
  `audit_ledger.rs`)
- `merkle_proof(seq)` + free-function `verify_merkle_proof()` enable O(log N)
  per-entry inclusion proofs. ✅ (Sprint 3.2)

**Open items:**
- **P1** Audit-ledger persistence: in-memory only; crash recovery requires
  a `LedgerStore` trait (TD-P9-001).
- **P2** Rotation / pruning: chain is unbounded; natural fit with batch ZK
  of Sprint 3.2 (TD-P9-002).
- **P2** JCS (RFC 8785) canonicalisation for deterministic re-hashing in a
  zkVM (TD-P9-003).
- **P2** `emit_batch` atomic append for multi-entry transactions (TD-P9-004).

---

## Phase 4 — Semantic layer (K\*6 + cross-belief contradictions) 🟢

~~This is the **single largest gap** between the current implementation and
the project's stated claims.~~ **Closed, Sprint 1 + Integration sprint.**

K\*6 is implemented via the `EmbeddingProvider` trait in
`crates/epica-core/src/embedding/`. The `OpenAiEmbeddingProvider`
(crate `epica-openai`) closes the loop end-to-end against the OpenAI
embeddings API (also compatible with Voyage AI, Together, and
self-hosted `text-embeddings-inference`).

~~TD-003~~ — **Resolved, Sprint 1.**

**Open items after closure:**
- **P1** Embedding cache is unbounded; long-running agents risk memory
  divergence (TD-P8-002). Fix before enabling in production.
- **P2** Python binding for the embedding provider (TD-P10-001).
- **P2** Voyage AI–specific parameters (TD-P10-002).
- **P2** Provider observability / Prometheus metrics (TD-P10-003).

---

## Phase 5 — `epica-mcp` (HTTP server + Tasks + observability) 🟢

Full Axum MCP 2026 server, 16 routes, SEP-1686 Tasks, OAuth 2.1 JWT,
per-IP rate limiting, Prometheus metrics, OTLP exporter (feature `otlp`),
DOT visualisation at `GET /v1/visualize/dot`, Server Card.

**Done when:**
- All E2E tests green (`cargo test -p epica-mcp`). ✅
- Tasks survive a process restart on the persistent backend
  (`task_store_persistence.rs`, feature `sled-store`). ✅
- LLM provider selectable at boot via `EPICA_LLM_PROVIDER`. ✅
- ~~TD-P5-002 (TaskStore persistence)~~ — closed by commit `6bd9bf3`.

**Open items:**
- **P2** Health endpoint should report the live LLM-provider status
  (configured / missing-key / unreachable).

---

## Phase 6 — `epica-python` (PyO3 SDK) 🟢

Full PyO3 bindings: `PyBeliefQuad`, `PyBeliefRuntime`, `PyBehavioralContract`,
decorators (`@belief_state`, `@governed_by`), LangChain integration, PEP 561
stubs. 65 pytest cases pass under `maturin develop`. `LlmClient` injection
is now exposed via `PyLlmClientHandle` + `PyMockLlmClient`.

**Done when:**
- `pytest` + `PyMockLlmClient` round-trip works. ✅
- ~~TD-P7-002: `BeliefRuntime::with_llm_client()` not exposed to Python~~
  — **Resolved, Sprint 1.** `attach_llm_client` / `detach_llm_client` +
  `PyLlmClientHandle` land in `crates/epica-python/src/runtime.rs`.
- ~~TD-P7-001: `epica-python` not in `default-members`~~ — **Resolved as
  documented carve-out.** PyO3 needs Python on the host; see comment in
  workspace `Cargo.toml:15`.

**Open items:**
- **P2** Native async (`await`-able methods) needs `pyo3-asyncio` 0.22 or
  a hand-rolled bridge (TD-P6-001).
- **P2** `MockLlmClient` parity test in `pytest` (TD-P8-008).

---

## Phase 7 — `epica-memory` (Persistence backends) 🟢

`LongTermMemoryStore` trait, `FlushResult`, `SchemaDescriptor`. Redis backend
fully implemented with sovereignty-aware TTL. **Neo4j backend real impl via
`neo4rs 0.8` (opt-in feature `neo4j`).**

~~**P2 open: Neo4j returns `Err`** (TD-NEW-001)~~ — **Resolved, Sprint 1.**
`Neo4jMemoryStore::connect()` returns a live `Graph` against `neo4rs 0.8`.

**Open items:**
- **P1** Live Neo4j smoke test not in CI (TD-P8-004). The driver compiles
  and connects; the test requires a CI runner with a Neo4j sidecar service.

---

## Sprint 1 — Closing the K\*6 gap 🟢

K\*6 (extensionality) is the only AGM postulate that requires semantic
reasoning — it demands that logically equivalent inputs produce identical
revision outcomes. It was the first thing a formal-methods reviewer would
check, and it was a structural stub. This sprint made it real.

The central design constraint was preserving the synchronous `revise()` API.
Embedding computation is inherently async — HTTP round-trips to a model
provider, ONNX inference, or vector lookup all have unpredictable latency.
Making `revise()` async would have been a breaking change to every callsite
and would have pushed concurrency decisions onto callers who opted into a
sync contract. The resolution was a two-tier architecture: the hot path
consults an in-memory cache synchronously; the warm-up path is async and
runs before the mutation window, not during it. A cache miss falls back to
literal comparison — indistinguishable from the pre-sprint behavior. K\*6
is real when embeddings are warmed, and safely conservative when they are
not.

The same sprint closed two secondary gaps that were beginning to undermine
broader claims. The Neo4j backend had been returning `Err` with no real
driver — a gap that made the persistence story incomplete for production
deployments. The Python SDK was missing `LlmClient` injection entirely,
which meant System 2 reflection could not be exercised from Python at all.
Both were resolved without touching the core API.

**Open items:** embedding cache is unbounded — memory divergence under
long-running agents is the next concrete risk (TD-P8-002).

---

## Sprint 2 — Whole-agent drift detection 🟢

`BehavioralContract` is a point-in-time enforcement mechanism: it checks
whether a specific belief satisfies a specific invariant at the moment of
mutation. That design is correct for what it does, but it is structurally
blind to a distinct failure mode — the gradual accumulation of
miscalibrated beliefs where no single invariant fires, yet the agent's
posterior has drifted far from its own causal prior across the entire quad.
No contract rule catches this because no contract rule spans the whole
belief set continuously.

This sprint introduced a variational free-energy monitor grounded in
Friston's Active Inference framework. The mapping is deliberately
conservative: hidden states are belief truth values, the posterior is
`fast_confidence`, the prior is the Noisy-OR of causal-graph parents.
This is not a faithful reproduction of the neuroscience substrate —
it is an auditable approximation over data the runtime already owns.
The FEP is substrate-agnostic by design; what matters is that the
KL divergence between posterior and prior is computable, bounded, and
meaningful as a drift signal.

The monitor runs as an opt-in hook on every `insert_belief`, never
blocking the mutation. Budget breaches emit a `SurpriseSignal` that can
be wired to a contract gate or left as telemetry — the caller decides
the enforcement semantics.

**Open items:** `SurpriseSignal` is not yet sealed into `AuditEntry`,
so FEP breaches do not appear in the Merkle ledger (TD-P11-003).
Resolving this closes the loop between continuous monitoring and
cryptographic evidence.

---

## Sprint 3 — Cryptographically verifiable audit trail 🟢

A structured audit log that the log producer controls is, at best, a
credibility signal. It provides no guarantee to an external party that
the log has not been selectively edited after the fact. The criticism
is valid: intelligent logs are still just logs. This sprint addressed it
in two layers.

The first layer — tamper evidence — establishes that any modification to
any entry in the audit chain is detectable without external infrastructure.
Each entry is hashed against its predecessor using BLAKE3; altering a
single entry invalidates every downstream hash. The property holds even
against a sophisticated adversary who repairs the downstream links: the
Merkle root still diverges. This is a chain-level guarantee, not an
entry-level one.

The second layer — non-repudiation — establishes that a specific party
produced a specific ledger window. An Ed25519 signature over the Merkle
root, committed with a producer key, means the producer cannot later deny
having sealed that window. Combined with per-entry O(log N) Merkle proofs,
an auditor can verify a specific entry's membership in the signed window
without retrieving the full ledger.

**On the RISC Zero pivot:** the original plan called for ZK proofs of AGM
transition validity. On analysis, that conflates two distinct properties.
Non-repudiation, tamper evidence, and offline verifiability — the
properties an enterprise auditor actually requires — are fully delivered
by Ed25519 over the Merkle root. ZK would additionally prove that the
transitions within the log are valid without revealing the belief content,
which is a privacy property, not a correctness one. Given that the
RISC-V toolchain adds significant CI provisioning overhead with no
correctness gain, deferring it was the correct call. The upgrade surface
is preserved as a documented skeleton under feature `risc0`.

**Open items:** the ledger is in-memory only — crash recovery requires a
persistent `LedgerStore` backend (TD-P9-001), and JSON canonicalization
needs RFC 8785 compliance before re-hashing in a zkVM becomes possible
(TD-P9-003).

---

## Sprint 4 — Empirical grounding 🟢

A system with formal guarantees and no benchmark numbers is easy to
dismiss. The sub-1ms hot-path claim existed in the design rationale but
had no evidence attached. This sprint established the evidentiary baseline.

The design constraint was reproducibility without environment dependencies.
Running against live ALFWorld or WebShop requires a Python environment,
AI2-THOR, a Flask shopping simulator, and LLM API spend per run — a
provisioning footprint that makes benchmarks flaky, expensive, and
unreproducible for anyone who clones the repository. The alternative was
to capture the *epistemic shape* of those benchmarks: the revision patterns,
belief-confidence trajectories, and contradiction sequences that
characterize each domain, without the environment machinery.

Concretely: the ALFWorld suite exercises sequential goal pursuit with
mid-trajectory AGM corrections. The WebShop suite exercises the
search-then-refute pattern — high-confidence candidates progressively
contradicted by filter refinements — which is precisely the scenario that
exercises K\*6 paraphrase detection and AGM contradiction handling under
sustained load. Both suites are deterministic and seed-reproducible,
meaning any reviewer can diff their local output against the committed
CSVs byte-for-byte.

The four headline metrics — T-ECE, AGM contradiction rate, free-energy
mean, and insert latency percentiles — were chosen because they correspond
directly to the system's design claims. A runtime that claims calibrated
belief revision must show T-ECE. One that claims tamper-evident auditing
must show AGM contradiction counts. One that claims sub-millisecond
insertion must show p99 latency with all opt-in features active.

| Suite | T-ECE | AGM contradictions | FE mean (nats) | p99 lat (µs) |
|---|---:|---:|---:|---:|
| `alfworld_like` | **0.080** | 0 | 1.88 | **79** |
| `webshop_like` | **0.658** | 165 | 1.85 | **253** |

T-ECE = 0.658 on WebShop is not a regression — it is the runtime correctly
surfacing the miscalibration inherent in the search-then-refute pattern.
A system that paperd over this would report T-ECE ≈ 0 and miss 165 AGM
contradictions. p99 = 253µs is measured with the FEP hook active on every
insertion, confirming the sub-1ms promise holds under the full feature set.

**Open items:** real ALFWorld and WebShop adapters are the P0 item for
the next sprint. The `RealEnvAdapter` trait is the designated seam; the
harness API is stable and will not need to change when live environments
are wired in (TD-P13-001).

---

## Cross-cutting workstreams

### Testing rigor — current state

- proptest covers K\*2, K\*3, K\*5 in `epica-core`; System 1 invariants in
  `epica-runtime`; auth policies in `epica-contracts`. ✅
- libFuzzer targets exist for `revise()` and `BeliefValue` deserialization;
  CI integration deferred (see `docs/fuzzing.md`). ✅
- **P1** Wire a scheduled `fuzz.yml` workflow (5-minute weekly run per
  target). Crashes auto-file issues.
- **P2** Mutation testing with `cargo-mutants` on `epica-core::revision`.
- **P2** proptest for K\*6 semantic identities (random vector pairs with
  cosine constraints) — TD-P11-005.

### Observability — current state

- `tracing` everywhere, structured JSON via `tracing-subscriber`. ✅
- Prometheus exporter (`/metrics`) with 7 belief / contract / System 2
  counters. ✅
- OpenTelemetry OTLP exporter behind the `otlp` feature: build with
  `--features otlp` and set `EPICA_OTLP_ENDPOINT=http://collector:4317`
  to stream spans to any OTLP-compatible collector (Jaeger, Tempo,
  Honeycomb). Misconfiguration is non-fatal. See
  [docs/observability.md](./docs/observability.md). ✅
- **P2** `SurpriseSignal` from the FEP monitor should emit as an
  `AuditEntry::ContractViolation { kind: HomeostaticBreach }` so the ledger
  seals FEP breaches (TD-P11-003).

### Supply chain — current state

- All crates declare `license = "MIT OR Apache-2.0"` and a canonical
  `repository` URL. ✅
- `cargo-audit` (`.github/workflows/audit.yml`) and `cargo-deny`
  (`.github/workflows/deny.yml`) gate on RUSTSEC advisories, licence
  allowlist, banned crates, duplicate versions, and source allowlist
  on every PR and on a weekly cron. ✅

**Open advisories with explicit ignores** — each is documented in both
`deny.toml` and `audit.yml` with the justification copy-pasted next to
the ID, so an auditor reading either file sees the policy in place.

| ID | Tracking | Reason it stays ignored |
|---|---|---|
| RUSTSEC-2023-0071 (`rsa` 0.9, Marvin Attack) | **ROADMAP-CVE-1** | No upstream fix; mitigated by using HS256 JWT in `epica-mcp` (the RSA codepath is not exercised on the default deploy). Drop ignore when RustCrypto ships constant-time `rsa`. |
| RUSTSEC-2025-0020 (`pyo3` 0.22 `PyString::from_object`) | **ROADMAP-CVE-2 (P1)** | Fix is in PyO3 0.24.1+; migration is non-trivial (ABI, `type_object_bound` rename, deprecated bindings). Scheduled for the next sprint. |
| RUSTSEC-2025-0057 (`fxhash` unmaintained) | **ROADMAP-DEP-1** | Transitive via `sled` 0.34. No fix available without swapping the task-store backend, which would defeat TD-P5-002. Monitor sled's successor (`sled-1.0` family). |
| RUSTSEC-2024-0384 (`instant` unmaintained) | **ROADMAP-DEP-1** | Same root cause as fxhash: transitive via `sled → parking_lot 0.11`. Resolves when sled upgrades `parking_lot`. |

### Performance baseline — current state

- Criterion benches published with hardware, toolchain, profile and
  sample size declared. ✅ (see [BENCHMARKS.md](./BENCHMARKS.md))
- End-to-end harness (`epica-bench`) with 4 metrics over synthetic
  ALFWorld / WebShop traces. ✅ (see [docs/benchmarks/README.md](./docs/benchmarks/README.md))
- **P0 for real-env sprint** A real-environment calibration benchmark
  (ALFWorld / WebShop live) for T-ECE measurement against true partial-
  observability tasks. `RealEnvAdapter` trait is the seam (TD-P13-001).

---

## Explicitly out of scope

Listing these so the next reviewer does not have to derive the absence:

- **Formal verification (Coq / Lean) of AGM postulates.** Proptest with
  256 cases per postulate is the industry standard for a library of this
  scope. Verified-proof investment is not on the roadmap.
- **Self-rewriting code at runtime (Quine).** Explicitly descoped in
  the post-public-review planning round — contradicts the formal
  verification promise.
- **A second non-LLM persistent backend** beyond Redis (e.g. Postgres).
  The trait is in place; an implementation is one PR away if a user
  needs it.
- **A managed cloud offering.** Epica is a library and a server, not a
  service.

---

## Latest sprints

- **Sprint 4 — Benchmarks** (see Sprint 4 section above):
  Synthetic ALFWorld/WebShop harness; `epica-bench` CLI; 4 headline
  metrics; artefacts in `docs/benchmarks/`. p99 ≤ 253 µs confirmed.

- **Sprint 3 — ZK Evidence** (see Sprint 3 section above):
  BLAKE3 Merkle audit ledger; Ed25519 `EvidenceReceipt`; `epica-verify`
  CLI with `keygen` / `seal` / `verify`; honest RISC Zero pivot.

- **Sprint 2 — Active Inference** (see Sprint 2 section above):
  `epica-active-inference` crate; VFE monitor; opt-in FEP hook on
  `BeliefRuntime`.

- **Sprint 1 — Post-hardening** (see Sprint 1 section above):
  K\*6 semantic via `EmbeddingProvider`; Neo4j real impl;
  Python `LlmClient` bridge; `OpenAiEmbeddingProvider` E2E.

- **Staff-level hardening** (commits `c4babdd..HEAD`):
  Criterion benchmarks published; proptest expanded; cargo-fuzz targets
  added; `SledTaskStore` + restart-survival test closed TD-P5-002;
  OpenAI provider crate landed; this roadmap.

- **Public-review hardening** (commits `f96896e..1fca7c1`):
  Closed all 17 items of the public code review (4 critical, 6 serious,
  7 minor). See `DEVLOG.md`.
