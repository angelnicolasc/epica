# Phase Roadmap

> The operational state of the project. Documents what is implemented,
> what is verified, and what gaps remain — per phase and per
> post-hardening sprint. For claimed capabilities without a verification
> path listed, treat the claim as unverified until tested.
>
> Updated for the post-public-review hardening (Sprints 1–4) and the
> integration step that closes TD-P8-001. The canonical change log lives
> in [DEVLOG.md](../DEVLOG.md).

## Status legend

| Symbol | Meaning |
|---|---|
| ✅ **Implemented** | Compiled, tested, no `todo!()` stubs |
| 🟡 **Implemented (partial)** | Compiled, tested, with known functional gaps documented inline |
| 🔵 **Skeleton** | Trait + tests + docs landed; concrete backend deferred behind a feature with the cost/value rationale in DEVLOG |
| 🟥 **Returns `Err`** | Graceful error; feature not available |

---

## Phase status (Phases 1–7 + post-hardening Sprints 1–4)

| Phase / Sprint | Crate(s) | Status | What is implemented | Known gaps |
|---|---|---|---|---|
| 1 | `epica-core` | ✅ | BeliefQuad, 4 graphs, **AGM K\*2–K\*6 as hard errors**, System 1 Noisy-OR with cycle guard, checkpoint/rollback, diff, counterfactual, T-ECE pipeline, PageRank centrality, `EmbeddingProvider` trait | K\*6 semantic-path applies to `Asserted/Asserted` only (TD-P8-003) |
| 2 | `epica-runtime` | ✅ | `BeliefRuntime`, dual-process System 1+2 (async via `System2Pending` + spawn), `TokenBucket` with real refill, `ConfidenceHistory`, `SessionReport`, multicriteria retrieval, T-ECE computation | — |
| 3 | `epica-contracts` | ✅ | `BehavioralContract` C=(P,I,G,R), `ContractEngine`, `GovernanceTracker`, all 9 Mnemonic Sovereignty primitives, drift-bound via CLT, TOML-deserializable `ContractConfig`, **`AuditLedger` Merkle chain (BLAKE3)** | — |
| 4 | `epica-macros` | ✅ | `#[derive(BeliefState)]` with 9 attributes; trybuild expansion tests | — |
| — | `epica-anthropic` | ✅ | `AnthropicLlmClient` + `ProspectiveClient`; retry policy (3 attempts, exp backoff) | Live calls require `ANTHROPIC_API_KEY` |
| — | `epica-openai` | ✅ | `OpenAiLlmClient` + **`OpenAiEmbeddingProvider`** with batching + retry; OpenAI-compatible (Voyage, Together, TEI self-hosted) | Live calls require `OPENAI_API_KEY` |
| 5 | `epica-mcp` | ✅ | Full Axum MCP 2026 server, 16 routes, SEP-1686 Tasks, OAuth 2.1 JWT (HS256 + RS256), per-IP rate limiting (`governor`), Prometheus metrics, Server Card, SSE for tasks | Task store is in-memory unless `--features sled-store` (resolved TD-P5-002) |
| 6 | `epica-python` | ✅ | Full PyO3 SDK: `PyBeliefQuad`, `PyBeliefRuntime`, `PyBehavioralContract`, decorators, integrations, **`LlmClient` injection via `PyLlmClientHandle` + `PyMockLlmClient`** | No async `await` bridge (TD-P6-001); not in `default-members` (resolved as carve-out, see Cargo.toml) |
| 7 | `epica-memory` | ✅ | `LongTermMemoryStore` trait, Redis (full, sovereignty-aware TTL), **Neo4j real impl via `neo4rs 0.8`** (opt-in feature) | Live Neo4j smoke test not in CI yet (TD-P8-004) |
| **Sprint 1** | `epica-core` | ✅ | **K\*6 semantic via `EmbeddingProvider`**; cosine bands; verdict trace + witness | Documented in [DEVLOG 2026-05-18 Sprint 1] |
| **Integration** | `epica-openai` | ✅ | **`OpenAiEmbeddingProvider` end-to-end** with `BeliefQuad` — closes TD-P8-001 | TD-P10-001 Python binding pending |
| **Sprint 2** | `epica-active-inference` | ✅ | **Variational free-energy monitor** (Friston / pymdp lineage). `ActiveInferenceMonitor::observe(quad, node)`; hook opt-in on `BeliefRuntime` via feature | TD-P11-001..005 (Beta posteriors, Criterion bench, AuditEntry emission, TOML config, proptest) |
| **Sprint 3.1** | `epica-contracts` | ✅ | **`AuditLedger` Merkle chain + `merkle_proof` per-entry** (BLAKE3); opt-in on `AuditPolicy::with_ledger()` | TD-P9-001..004 (persistence, rotation, JCS canonicalisation, batch atomicity) |
| **Sprint 3.2** | `epica-zk-evidence` | ✅ | **`EvidenceReceipt` wire format + `Ed25519Prover`/`Verifier`** with 4-layer verification (length/range/root, signature, inclusions) | RISC Zero ZK proof of AGM trace is `risc0` feature **skeleton only** — [`zk_skeleton.rs`](../crates/epica-zk-evidence/src/zk_skeleton.rs); TD-P12-006 |
| **Sprint 3.3** | `epica-zk-evidence` (bin) | ✅ | **`epica-verify` CLI** with `keygen`, `seal`, `verify` subcommands; CSV-/JSON-friendly | TD-P12-002 (stdin/stdout streaming), TD-P12-004 (key rotation), TD-P12-005 (`BeliefRuntime::snapshot_ledger()`) |
| **Sprint 4** | `epica-benchmarks` | ✅ | **Synthetic ALFWorld/WebShop traces + `epica-bench` CLI**; 4 metrics reported (T-ECE, violations, FE mean, latency p50/p95/p99); CSV + Markdown out to `docs/benchmarks/` | 🔵 **Real ALFWorld / WebShop adapters** are documented `RealEnvAdapter` trait — [`real_adapters.rs`](../crates/epica-benchmarks/src/real_adapters.rs); TD-P13-001..005 |

---

## Verification per phase

### Phase 1 — verified

```bash
cargo check  -p epica-core
cargo test   -p epica-core
cargo clippy -p epica-core
```

Verified outputs:
- All AGM postulate tests pass — including the 4 K\*6 paraphrase / witness cases.
- `system1_propagation` integration test passes.
- `quad_basic` integration test passes (insert, remove, checkpoint, rollback).
- `embedding/` module's KL / cosine / threshold tests pass.

### Phase 2 — verified

```bash
cargo check -p epica-runtime --features system2,active-inference
cargo test  -p epica-runtime --features system2,active-inference
```

Verified outputs:
- `system1_only.rs` (5), `system2_mock.rs` (4), `tece_session.rs` (4),
  `beliefshift_benchmark.rs` (2 — pipeline + variable confidence),
  `contracts.rs` (8), `system1_invariants.rs` (5 proptest),
  **`active_inference_hook.rs` (5)**.

### Phase 3 — verified

```bash
cargo test -p epica-contracts
```

Verified outputs:
- 34 lib unit tests + 21 sovereignty + **8 audit_ledger integration** +
  5 contract_proptest.
- Audit-ledger cases include tamper detection (entry hash mismatch +
  chain link broken), Merkle root divergence under forgery, shared
  ledger aggregation, schema-versioned wire format.

### Phase 4 — verified

```bash
cargo test -p epica-macros
cargo test -p epica-anthropic
```

Verified outputs:
- 8 unit + 2 trybuild expansion (`basic.rs`, `full_attrs.rs`).
- AnthropicLlmClient + ProspectiveClient compile and retry policy
  works against wiremock.

### Phase 5 — verified

```bash
cargo check -p epica-mcp
cargo test  -p epica-mcp

# Manual smoke:
EPICA_NO_AUTH=1 cargo run --bin epica-serve
curl http://localhost:8765/health
curl http://localhost:8765/.well-known/epica-server-card.json | python -m json.tool
curl -X POST http://localhost:8765/v1/beliefs \
     -H 'Content-Type: application/json' \
     -d '{"key":"user_intent","value":"refactor auth","confidence":0.9}'
curl http://localhost:8765/metrics
```

Verified outputs:
- 29 E2E tests across `e2e_belief_lifecycle.rs` (8) +
  `e2e_checkpoint_rollback.rs` (5) + `e2e_health.rs` (6) +
  `e2e_query.rs` (6) + `e2e_tasks.rs` (4).

### Phase 6 — verified (requires Python environment)

```bash
cd crates/epica-python
maturin develop
python -m pytest tests/ -v   # 65 tests
```

Verified outputs:
- `test_belief_quad.py` (22), `test_runtime.py` (14),
  `test_contracts.py` (14), `test_decorators.py` (9),
  `test_e2e.py` (6).

### Phase 7 — verified (Redis full; Neo4j compile-verified)

```bash
cargo check -p epica-memory
cargo check -p epica-memory --features neo4j     # real driver compiles

cargo test -p epica-memory --features redis      # requires running Redis
```

Neo4j: `Neo4jMemoryStore::connect()` returns a live `Graph` against
`neo4rs 0.8`; the unit tests verify compilation; live-server smoke is
TD-P8-004 (deferred until a CI runner provisions Neo4j).

### Sprint 1 + Integration — verified

```bash
# K*6 semantic with the real provider end-to-end
cargo test -p epica-openai --test embeddings
# Includes: k6_semantic_paraphrase_works_against_warmed_provider
```

### Sprint 2 — verified

```bash
cargo test -p epica-active-inference          # 16 unit tests
cargo test -p epica-runtime --features active-inference \
           --test active_inference_hook       # 5 integration tests
```

### Sprint 3.1 — verified

```bash
cargo test -p epica-contracts                 # incl. 8 audit-ledger integration
```

### Sprint 3.2 + 3.3 — verified

```bash
cargo test -p epica-zk-evidence              # 23: 18 unit + 4 CLI smoke + 1 doctest
target/debug/epica-verify --help             # subcommands: keygen, seal, verify

# Full E2E:
target/debug/epica-verify keygen --secret-out /tmp/sec.hex
# ... produce ledger.json from a runtime session ...
target/debug/epica-verify seal --ledger ledger.json --secret /tmp/sec.hex --out receipt.json
target/debug/epica-verify verify --ledger ledger.json --receipt receipt.json
```

### Sprint 4 — verified

```bash
cargo test -p epica-benchmarks               # 23 tests
target/release/epica-bench run-all --trajectories 200 --out-dir docs/benchmarks
```

The CSVs in `docs/benchmarks/` were produced by the command above and
are reproducible byte-for-byte from the same trajectory count.

---

## Open technical debts (post-Sprint-4)

The canonical list lives in `DEVLOG.md`. Summary table below; items
marked **bumped** had their priority raised by Sprint-4 work that
brought them closer to the critical path.

| ID | Crate | Description | Impact | Note |
|---|---|---|---|---|
| TD-P8-002 | `epica-core` / `epica-openai` | Embedding cache unbounded | Memory diverges in long-running agents | **Bumped**: Sprint-4 benchmarks make this concrete pre-real-env. |
| TD-P8-003 | `epica-core` | K\*6 semantic path applies to `Asserted/Asserted` only | Inferred-variant paraphrases unhandled | Documented; doc-only fix unless future use case demands it. |
| TD-P8-004 | `epica-memory` | Live Neo4j smoke test not in CI | Driver compiles but isn't exercised against a real server | Resolves when CI gains a Neo4j sidecar service. |
| TD-P8-008 | `epica-python` | `MockLlmClient` parity test in `pytest` | Rust-side unit / integration cover it; Python wiring untested | Add a `pytest` once `maturin develop` is in CI. |
| TD-P9-001..004 | `epica-contracts` | Audit-ledger persistence / rotation / JCS canonicalisation / batch atomicity | Multi-process audit + future zkVM hashing | Single-process semantics fully covered today. |
| TD-P9-005 | `epica-contracts` | Per-entry Merkle proof | **RESOLVED** (Sprint 3.2) | — |
| TD-P10-001..003 | `epica-openai` / `epica-anthropic` | Python binding of embedding provider; Voyage-specific params; provider observability | Multi-tenant deploys | Optional uplift. |
| TD-P11-001..005 | `epica-active-inference` | Beta posteriors; Criterion bench; AuditEntry emission; TOML config; proptest identities | FEP rigor + integration with audit ledger | Sprint-2 follow-on. |
| TD-P12-001..006 | `epica-zk-evidence` | `LedgerEntry: Deserialize`; CLI stdin/stdout; JCS canonicalisation; key rotation; ledger-snapshot helper; **RISC Zero backend** | UX polish + future zkVM | None blocks portfolio. |
| TD-P13-001..005 | `epica-benchmarks` | **Real ALFWorld/WebShop adapters**; ledger sealing in bench; System 2 + LLM bench; PNG plots; Criterion FEP `observe()` bench | Sprint-4 follow-on | Real adapters are the headline item; require Python infra. |
| TD-NEW-001 | `epica-memory` | Neo4j driver real impl | **RESOLVED** (Sprint 1) | — |
| TD-NEW-002 | `epica-anthropic` | Concrete `LlmClient` impl | **RESOLVED** (Phase 2) | — |
| TD-P7-001 | workspace | `epica-python` in `default-members` | **RESOLVED as documented carve-out** | PyO3 needs Python on the host — see `Cargo.toml` comment. |
| TD-P7-002 | `epica-python` | LLM client injection from Python | **RESOLVED** (Sprint 1) | `PyLlmClientHandle` + `PyMockLlmClient`. |
| TD-003 | `epica-core` | Semantic contradiction via embeddings | **RESOLVED** (Sprint 1 + Integration) | `EmbeddingProvider` + `OpenAiEmbeddingProvider`. |

A debt closed since the public review is left in the table with the
resolution noted, so the trajectory is visible at a glance.

---

## What no phase covers (by design)

These are deliberately out of scope and listed so a reviewer doesn't
have to derive the absence:

- **Formal proof of AGM compliance in Coq / Lean.** Proptest with 256+
  cases per postulate is the industry standard for a library of this
  scope. Verified-proof investment is not on the roadmap.
- **Distributed transaction semantics across multiple `BeliefRuntime`
  instances.** Single-process consistency is provided by `RwLock`.
  Cross-process state is out of scope.
- **Self-rewriting code at runtime (Quine).** Explicitly descoped in
  the post-public-review planning round — contradicts the formal
  verification promise.
- **A managed cloud offering.** Epica is a library and a server, not a
  service.
