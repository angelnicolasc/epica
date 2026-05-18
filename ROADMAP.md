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
| 🔵 | Designed. Interface drafted, implementation deferred to a future sprint. |
| ⚪ | Planned. Listed for visibility; no design yet. |

| Priority | Meaning |
|---|---|
| **P0** | Required for the next sprint. Blocks elevation of dependent phases. |
| **P1** | Important. Ships when P0 is clear. |
| **P2** | Nice to have. Will land if bandwidth allows. |

Open items reference IDs in `DEVLOG.md` (`TD-XXX`, `HD-XXX`) where they have a
fuller diagnostic. When a TD is closed in code, the matching row here is
struck through and the closing commit linked.

---

## Phase 1 — `epica-core` (BeliefQuad + AGM + System 1) 🟢

The mathematical core: four orthogonal graphs over a shared `SlotMap`, AGM
revision satisfying postulates K*2–K*5 as hard errors in every build,
System 1 Noisy-OR propagation, checkpoint + rollback with K*4 guard,
diff and counterfactual, multicriteria retrieval.

**Done when:**
- `cargo test -p epica-core` is green and includes proptest cases for K*2,
  K*3, K*4, K*5 (≥ 256 cases each). ✅
- `cargo bench -p epica-core` produces reproducible Criterion reports for
  insert / revise / checkpoint+rollback / System 1 propagation across
  100 / 1 000 / 10 000 nodes. Reported in [BENCHMARKS.md](./BENCHMARKS.md). ✅
- Two libFuzzer targets (`fuzz_revise`, `fuzz_belief_value`) build under
  `cargo +nightly fuzz build`. ✅ (see [docs/fuzzing.md](./docs/fuzzing.md))

**Open items:**
- **P0** K*6 (extensionality) is currently structural-only. The full
  postulate requires deciding logical equivalence of belief values, which
  in turn requires semantic embedding comparison. Tracked as TD-003 in
  `DEVLOG.md` and pulled into Phase 4 below.
- **P1** Pathological-chain System 1 propagation cost (`~374 µs / node` at
  depth 10 000, see BENCHMARKS.md). Real DAGs are shallow, but the
  recursion + `descendants_of()` walk is amenable to iterative rewrite.
  Tracked as HD-S2-A1 in DEVLOG.

## Phase 2 — `epica-runtime` (Dual-process + retrieval + T-ECE) 🟢

`BeliefRuntime` over an `Arc<RwLock<BeliefQuad>>` with the dual-process
state machine, `TokenBucket` reflection budget, `ConfidenceHistory` for
T-ECE, multicriteria retrieval.

**Done when:**
- `cargo test -p epica-runtime` is green. ✅
- `update_belief()` returns `System2Pending` rather than blocking on the
  LLM. ✅ (sprint 1, commit `2e0cbce`)
- Property tests assert `fast_confidence ∈ [0, 1]` and strict version
  monotonicity over arbitrary mutation sequences. ✅

**Open items:**
- **P1** No System 1 / System 2 throughput benchmark when an `LlmClient`
  is wired up (only mock-client integration tests exist today). Needs a
  fixture LLM with controlled latency.

## Phase 3 — `epica-contracts` (Behavioral C=(P,I,G,R) + Sovereignty) 🟢

`BehavioralContract`, `ContractEngine`, `GovernanceTracker`, all nine
Mnemonic Sovereignty primitives, drift-bound computation via CLT, TOML
`ContractConfig`.

**Done when:**
- `cargo test -p epica-contracts` is green and covers the four auth modes
  (allowlist / denylist / human-approval / permissive) with proptest cases. ✅

**Open items:** none open at this priority.

## Phase 4 — Semantic layer (K*6 + cross-belief contradictions) 🔵

This is the **single largest gap** between the current implementation and
the project's stated claims, and the most important next sprint.

**P0 — Implementation plan (to design before starting):**

- Embedding backend: shortlist `candle` (pure Rust, model on disk), `ort`
  (ONNX runtime), or remote API (`text-embedding-3-small`). Decision
  artefact required before opening the sprint.
- Default model: a 256–384 dim sentence embedder, MIT-licensed when the
  backend is local. Threshold for cosine-similarity contradiction: target
  ≥ 0.85 on `Contradicts` and ≤ 0.4 on randomly-paired beliefs.
- Surface change: `BeliefQuad::check_contradiction()` and
  `PostulateAudit::extensionality` consult the embedder when present and
  fall back to structural comparison otherwise. Same external contract.
- Acceptance: `tests/agm_postulates/k6_extensionality.rs` adds a proptest
  generating paraphrase pairs (templated) and validates the new path; the
  cross-belief contradiction integration test asserts an `InferredFrom`
  chain is broken correctly.

**Status:** designed in scope here, not implemented. TD-003 stays open
until the embedding sprint lands.

## Phase 5 — `epica-mcp` (HTTP server + Tasks + observability) 🟢

Full Axum MCP 2026 server, 16 routes, SEP-1686 Tasks, OAuth 2.1 JWT,
per-IP rate limiting, Prometheus metrics, Server Card.

**Done when:**
- All E2E tests green (`cargo test -p epica-mcp`). ✅
- Tasks survive a process restart on the persistent backend
  (`task_store_persistence.rs`, feature `sled-store`). ✅ (sprint 2)
- LLM provider selectable at boot via `EPICA_LLM_PROVIDER`, with at
  least two providers shipped (`anthropic`, `openai`). ✅ (sprint 2)
- ~~TD-P5-002 (TaskStore persistence)~~ — closed by commit `6bd9bf3`.

**Open items:**
- **P1** OpenTelemetry OTLP exporter. The `tracing` and Prometheus
  pipelines are live; OTLP export is the only missing observability hop
  for distributed tracing.
- **P2** Health endpoint should report the live LLM-provider status
  (configured / missing-key / unreachable).

## Phase 6 — `epica-python` (PyO3 SDK) 🟡

Full PyO3 bindings: `PyBeliefQuad`, `PyBeliefRuntime`, `PyBehavioralContract`,
decorators (`@belief_state`, `@governed_by`), LangChain integration, PEP 561
stubs. 65 pytest cases pass under `maturin develop`.

**Open items:**
- **P1** `BeliefRuntime::with_llm_client()` is not exposed to Python
  (TD-P7-002). System 2 reflection cannot be driven from Python today;
  callers must use the MCP HTTP layer instead. Resolving this requires
  bridging `Arc<dyn LlmClient>` across the FFI boundary.
- **P2** Native async (`await`-able methods) needs `pyo3-asyncio` 0.22 or
  a hand-rolled bridge over `tokio::runtime::Handle` (TD-P6-001).
- **P2** Auto-generated `.pyi` stubs from a `pyi` cargo subcommand to
  replace the hand-maintained file (TD-P6-003).

## Phase 7 — `epica-memory` (Persistence backends) 🟡

`LongTermMemoryStore` trait, `FlushResult`, `SchemaDescriptor`. Redis
backend fully implemented with sovereignty-aware TTL.

**Open items:**
- **P2** Neo4j backend returns `Err` (TD-NEW-001). No maintained Rust
  driver is available. The likely resolution is to swap Neo4j for
  SurrealDB (Rust-native, embedded or remote) and rename the feature.
  Worth flagging in any external "supports Neo4j" claim until decided.

---

## Cross-cutting workstreams

These do not belong to a single phase but are tracked here so they remain
visible.

### Testing rigor — current state

- proptest covers K*2, K*3, K*5 in `epica-core`; System 1 invariants in
  `epica-runtime`; auth policies in `epica-contracts`. ✅
- libFuzzer targets exist for `revise()` and `BeliefValue` deserialization;
  CI integration deferred (see `docs/fuzzing.md`). ✅
- **P1** Wire a scheduled `fuzz.yml` workflow (5-minute weekly run per
  target). Crashes auto-file issues.
- **P2** Mutation testing with `cargo-mutants` on `epica-core::revision`.

### Observability — current state

- `tracing` everywhere, structured JSON via `tracing-subscriber`. ✅
- Prometheus exporter (`/metrics`) with 7 belief / contract / System 2
  counters. ✅
- **P1** OTLP exporter wired behind a feature flag (`otlp`), default off.
  See Phase 5 above.

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
- **P1** A real-task calibration benchmark (ALFWorld / WebShop-style)
  for T-ECE measurement. The current `beliefshift_benchmark` validates
  the formula pipeline, not real-task calibration.

---

## Explicitly out of scope

Listing these so the next reviewer does not have to derive the absence:

- **Formal verification (Coq / Lean) of AGM postulates.** Proptest with
  256 cases per postulate is the industry standard for a library of this
  scope. Verified-proof investment is not on the roadmap.
- **A second non-LLM persistent backend** beyond Redis (e.g. Postgres).
  The trait is in place; an implementation is one PR away if a user
  needs it.
- **A managed cloud offering.** Epica is a library and a server, not a
  service.

---

## Latest sprints

- **Sprint 2 — Staff-level hardening** (commits `c4babdd..HEAD`):
  Criterion benchmarks published; proptest expanded; cargo-fuzz targets
  added; SledTaskStore + restart-survival test closed TD-P5-002; OpenAI
  provider crate landed with 4xx / 5xx retry-policy fix in
  `LlmClientError`; this roadmap.
- **Sprint 1 — Public-review hardening** (commits `f96896e..1fca7c1`):
  Closed all 17 items of the public code review (4 critical, 6 serious,
  7 minor). See `DEVLOG.md`.
