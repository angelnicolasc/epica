# Evidence

This document inventories what has been verified in this codebase, how
to reproduce each result, and which claims have no current empirical
backing. Updated for the post-public-review hardening (Sprints 1–4).

The summary: **~500 tests across 12 Rust crates pass on the default
workspace path**, plus 65 Python tests, plus 23 CLI smoke tests across
two binaries. Every test below has a reproduction command.

---

## Quick-start audit

```bash
# 1. Default workspace — every crate except epica-python
cargo test --workspace --exclude epica-python

# 2. Feature-gated paths
cargo check -p epica-memory          --features neo4j
cargo check -p epica-runtime         --features active-inference
cargo test  -p epica-runtime         --features active-inference
cargo check -p epica-zk-evidence     --features risc0    # documented skeleton

# 3. Python SDK
cd crates/epica-python && maturin develop
python -m pytest tests/ -v

# 4. Reproduce published bench artefacts
target/release/epica-bench run-all --trajectories 200 --out-dir docs/benchmarks
```

---

## Test inventory by crate

### `epica-core`

| Test suite | File | Tests | What is verified |
|---|---|---:|---|
| AGM postulates | [`tests/agm_postulates/`](../crates/epica-core/tests/agm_postulates/) | 14 | K\*2 success, K\*3 inclusion, K\*4 vacuity, K\*5 consistency, **K\*6 semantic equivalence (vacuous + paraphrase + anti-parallel + witness)**, minimality |
| Integration | [`tests/integration.rs`](../crates/epica-core/tests/integration.rs) | 7 | CRUD, rollback, System 1 Noisy-OR, propagation depth, checkpoint roundtrip |
| Embedding module | [`src/embedding/mod.rs`](../crates/epica-core/src/embedding/mod.rs) | inline | `NullEmbeddingProvider`, `CachedEmbeddingProvider`, cosine identities, threshold bands |
| Lib unit | various | 20 | Internal property tests, decay, retrieval scoring |

```bash
cargo test -p epica-core
```

### `epica-runtime`

| Test suite | File | Tests | What is verified |
|---|---|---:|---|
| System 1 only | [`tests/system1_only.rs`](../crates/epica-runtime/tests/system1_only.rs) | 5 | CRUD, rollback, retrieval ordering, TTL expiry |
| System 2 mock | [`tests/system2_mock.rs`](../crates/epica-runtime/tests/system2_mock.rs) | 4 | Activation, no-activation, throttle, slow_confidence update |
| T-ECE session | [`tests/tece_session.rs`](../crates/epica-runtime/tests/tece_session.rs) | 4 | History ground truth, `finalize_session()`, System 2 recording |
| BeliefShift benchmark | [`tests/beliefshift_benchmark.rs`](../crates/epica-runtime/tests/beliefshift_benchmark.rs) | 2 | **T-ECE = 0.07 < 0.08** on deterministic + variable-confidence scenarios |
| Contracts integration | [`tests/contracts.rs`](../crates/epica-runtime/tests/contracts.rs) | 8 | Precondition gating, invariants, governance |
| System 1 invariants (proptest) | [`tests/system1_invariants.rs`](../crates/epica-runtime/tests/system1_invariants.rs) | 5 | `fast_confidence ∈ [0, 1]`, version monotonicity |
| **Active-inference hook** (feature-on) | [`tests/active_inference_hook.rs`](../crates/epica-runtime/tests/active_inference_hook.rs) | 5 | Monitor observes each insert, budget breach signal, attach/detach |

```bash
cargo test -p epica-runtime --features system2,active-inference
```

### `epica-contracts`

| Test suite | File | Tests | What is verified |
|---|---|---:|---|
| Lib unit | various | 34 | Contract config, predicates, drift bounds, governance, sovereignty primitives, **ledger Merkle proof + tamper detection (10 cases)** |
| Sovereignty | [`tests/sovereignty.rs`](../crates/epica-contracts/tests/sovereignty.rs) | 21 | All 9 primitives end-to-end |
| Audit-ledger integration | [`tests/audit_ledger.rs`](../crates/epica-contracts/tests/audit_ledger.rs) | 8 | `emit()` seals, shared ledger aggregation, tamper detection, forgery → root divergence |
| Contract proptest | [`tests/contract_proptest.rs`](../crates/epica-contracts/tests/contract_proptest.rs) | 5 | Auth policies, recovery verification |

```bash
cargo test -p epica-contracts
```

### `epica-macros`

| Test suite | File | Tests | What is verified |
|---|---|---:|---|
| Unit | [`tests/belief_state_tests.rs`](../crates/epica-macros/tests/belief_state_tests.rs) | 8 | Insert count, round-trip, confidence defaults, typed accessors, TTL parsing, prospect flag, reflection threshold |
| Trybuild | [`tests/trybuild.rs`](../crates/epica-macros/tests/trybuild.rs) | 2 | Proc macro expansion (`basic.rs`, `full_attrs.rs`) |

```bash
cargo test -p epica-macros
```

### `epica-anthropic`

| Test suite | File | Tests | What is verified |
|---|---|---:|---|
| Lib + integration | various | 7 | Config, prompt building, response parsing, retry policy |

```bash
cargo test -p epica-anthropic
```

### `epica-openai`

| Test suite | File | Tests | What is verified |
|---|---|---:|---|
| Lib unit | various | 4 | Config parsing, embedding cache, batch clamp |
| LLM integration (wiremock) | [`tests/integration.rs`](../crates/epica-openai/tests/integration.rs) | 3 | Happy path, 429 retry, 401 non-retry |
| **Embeddings integration (wiremock)** | [`tests/embeddings.rs`](../crates/epica-openai/tests/embeddings.rs) | 7 | `warm_async` populates cache, sync `warm` queues, 429 retry, 401 non-retry, mismatch surfaces, batch split, **K\*6 paraphrase E2E with BeliefQuad** |

```bash
cargo test -p epica-openai
```

### `epica-active-inference`

| Test suite | File | Tests | What is verified |
|---|---|---:|---|
| Free-energy math | [`src/free_energy.rs`](../crates/epica-active-inference/src/free_energy.rs) | 8 | KL identity, non-negativity, finite under extremes, NoisyOr edge cases, F-total per quad, calibrated quad → KL ≈ 0, overconfidence → KL > 0.55 |
| Monitor | [`src/monitor.rs`](../crates/epica-active-inference/src/monitor.rs) | 7 | First observation has zero mean/std, quiet agent stays within budget, overconfidence trips budget, history bounded, reset, spike detector, capacity floor clamp |
| Config | [`src/config.rs`](../crates/epica-active-inference/src/config.rs) | inline | Default values, sanitised capacity |

```bash
cargo test -p epica-active-inference
```

### `epica-mcp`

| Test suite | File | Tests | What is verified |
|---|---|---:|---|
| Belief lifecycle | [`tests/e2e_belief_lifecycle.rs`](../crates/epica-mcp/tests/e2e_belief_lifecycle.rs) | 8 | Insert, get, update, round-trip, 404, provenance, multiple beliefs, readiness count |
| Checkpoint/rollback | [`tests/e2e_checkpoint_rollback.rs`](../crates/epica-mcp/tests/e2e_checkpoint_rollback.rs) | 5 | Checkpoint, rollback, invalid rollback, diff with T-ECE, unknown checkpoint |
| Health | [`tests/e2e_health.rs`](../crates/epica-mcp/tests/e2e_health.rs) | 6 | Liveness, readiness, server card fields, server card endpoints, JWKS, Prometheus |
| Query | [`tests/e2e_query.rs`](../crates/epica-mcp/tests/e2e_query.rs) | 6 | Empty query, query post-inserts, default budget, counterfactual 404, counterfactual surviving, contract status |
| Tasks | [`tests/e2e_tasks.rs`](../crates/epica-mcp/tests/e2e_tasks.rs) | 4 | Task 404, task without System 2, task poll structure, task SSE reachable |

```bash
cargo test -p epica-mcp
```

### `epica-memory`

| Test suite | File | Tests | What is verified |
|---|---|---:|---|
| Lib unit (Redis) | various (`--features redis`) | n/a | Sovereignty-aware TTL, namespace eviction |
| Neo4j (`--features neo4j`) | [`src/neo4j/mod.rs`](../crates/epica-memory/src/neo4j/mod.rs) | compile-test | Real `neo4rs 0.8` impl compiles end-to-end; smoke against live Neo4j is CI work (TD-P8-004) |

```bash
cargo check -p epica-memory --features neo4j
cargo test  -p epica-memory --features redis   # requires running Redis
```

### `epica-zk-evidence`

| Test suite | File | Tests | What is verified |
|---|---|---:|---|
| Receipt | [`src/receipt.rs`](../crates/epica-zk-evidence/src/receipt.rs) | 4 | Binding deterministic, binding changes under perturbation, hex round-trip, fixed-length rejection |
| Prover | [`src/prover.rs`](../crates/epica-zk-evidence/src/prover.rs) | 6 | Key distinctness, restore round-trip, secret length validation, seal whole-ledger root match, seal rejects out-of-range, seal with inclusions |
| Verifier | [`src/verifier.rs`](../crates/epica-zk-evidence/src/verifier.rs) | 8 | Happy path full + partial window, tampered signature, tampered ledger entry, shorter ledger, forged inclusion, wrong pubkey |
| **CLI smoke** | [`tests/cli_smoke.rs`](../crates/epica-zk-evidence/tests/cli_smoke.rs) | 4 | `keygen` + `seal` + `verify` round-trip, tampered ledger rejected, keygen refuses overwrite, default end |
| Doctest | `lib.rs` | 1 | Quick-start round-trip |

```bash
cargo test -p epica-zk-evidence
```

### `epica-benchmarks`

| Test suite | File | Tests | What is verified |
|---|---|---:|---|
| Traces | [`src/traces.rs`](../crates/epica-benchmarks/src/traces.rs) | 5 | Determinism, seed divergence, ALFWorld structure, WebShop paraphrase + purchase, suite slug round-trip |
| Metrics | [`src/metrics.rs`](../crates/epica-benchmarks/src/metrics.rs) | 4 | Empty samples → defaults, percentile cases, suite aggregation, FE optional |
| Harness | [`src/harness.rs`](../crates/epica-benchmarks/src/harness.rs) | 5 | ALFWorld trajectory T-ECE, WebShop contradictions, suite aggregation, FE on/off |
| Reporters | [`src/reporters.rs`](../crates/epica-benchmarks/src/reporters.rs) | 4 | Per-traj CSV, summary CSV, Markdown surface, latency edge cases |
| **CLI smoke** | [`tests/cli_smoke.rs`](../crates/epica-benchmarks/tests/cli_smoke.rs) | 4 | `run alfworld`, `run-all`, per-traj header+rows, `--no-active-inference` flag |
| Doctest | `lib.rs` | 1 | Quick-start `run_suite` |

```bash
cargo test -p epica-benchmarks
```

### `epica-python`

| Test suite | File | Tests | What is verified |
|---|---|---:|---|
| BeliefQuad | [`tests/test_belief_quad.py`](../crates/epica-python/tests/test_belief_quad.py) | 22 | CRUD, AGM revision, checkpoints, counterfactual, edges, dict protocol, dunders |
| Runtime | [`tests/test_runtime.py`](../crates/epica-python/tests/test_runtime.py) | 14 | Insert/get/update, retrieve, context manager, session report, T-ECE |
| Contracts | [`tests/test_contracts.py`](../crates/epica-python/tests/test_contracts.py) | 14 | Preconditions, invariants, severity, multiple constraints |
| Decorators | [`tests/test_decorators.py`](../crates/epica-python/tests/test_decorators.py) | 9 | `@belief_state`, `@governed_by` |
| E2E | [`tests/test_e2e.py`](../crates/epica-python/tests/test_e2e.py) | 6 | Full lifecycle, contract-governed, T-ECE bounded, decorator pipeline, counterfactual, retrieve ordering |

```bash
cd crates/epica-python && maturin develop
python -m pytest tests/ -v   # 65 tests
```

---

## Benchmark results

### Performance (Criterion, `epica-core`)

See [`BENCHMARKS.md`](../BENCHMARKS.md). Hot-path numbers:

| Operation | 10 000 beliefs | Per-node |
|---|---:|---:|
| `BeliefQuad::insert` (cumulative) | 8.24 ms | ≈ 824 ns |
| `BeliefQuad::revise` (cumulative) | 4.98 ms | ≈ 498 ns |
| `checkpoint → rollback_to` | 10.6 ms | ≈ 1.06 µs |

### End-to-end benchmark harness (`epica-benchmarks`)

See [`docs/benchmarks/README.md`](benchmarks/README.md). Numbers from
200 trajectories per suite:

| Suite | T-ECE | AGM contradictions | Free energy mean (nats) | p99 latency (µs) |
|---|---:|---:|---:|---:|
| `alfworld_like` | **0.080** | 0 | 1.88 | **79** |
| `webshop_like` | **0.658** | 165 | 1.85 | **253** |

**Reading WebShop's T-ECE = 0.658 as a good sign**: the trace
deliberately exercises the search-then-refute pattern (early
high-confidence candidates contradicted by later filters). A
well-functioning runtime *should* expose this miscalibration as T-ECE
≫ 0; that the metric catches it is the point.

```bash
target/release/epica-bench run-all --trajectories 200 --out-dir docs/benchmarks
```

---

## Verified claims

| Claim | Verification path | Status |
|---|---|---|
| AGM K\*2–K\*6 are hard errors | `cargo test -p epica-core --test agm_postulates` | ✅ |
| **K\*6 detects paraphrases (when provider warmed)** | `tests/agm_postulates/k6_extensionality.rs` + `epica-openai/tests/embeddings.rs::k6_semantic_paraphrase_works_against_warmed_provider` | ✅ |
| System 1 cycle-guarded Noisy-OR propagation | `tests/integration.rs::system1_propagation` | ✅ |
| T-ECE pipeline + variable confidence | `tests/beliefshift_benchmark.rs` | ✅ |
| Behavioral contracts gate every write | `cargo test -p epica-contracts` | ✅ |
| All 9 Mnemonic Sovereignty primitives enforce | `tests/sovereignty.rs` | ✅ |
| **Tamper-evident audit ledger** | `cargo test -p epica-contracts` (10 ledger cases) | ✅ |
| **Ed25519 receipt round-trips** | `cargo test -p epica-zk-evidence` (23 tests) | ✅ |
| **Active Inference monitor reports surprise** | `cargo test -p epica-runtime --features active-inference` + `cargo test -p epica-active-inference` | ✅ |
| MCP 2026 server, 16 routes, SEP-1686 Tasks | `cargo test -p epica-mcp` (29 E2E) | ✅ |
| OAuth 2.1 JWT (HS256 + RS256), JWKS rotation | `tests/e2e_health.rs` + `crates/epica-mcp/src/auth.rs` | ✅ |
| Python SDK with LLM client injection | `pytest` + `PyMockLlmClient` | ✅ |
| Real Neo4j backend (opt-in) | `cargo check -p epica-memory --features neo4j` | ✅ (compile-verified) |
| **Reproducible benchmark harness, 4 metrics** | `epica-bench run-all` → `docs/benchmarks/` | ✅ |

---

## Claims with no current empirical backing

Honest list of what's *not* tested against a live, real-world workload:

| Claim | Location | Status |
|---|---|---|
| ALFWorld / WebShop real-environment T-ECE | `docs/benchmarks/README.md` | **Synthetic only.** `RealEnvAdapter` trait is the seam (TD-P13-001). |
| ZK proof of AGM transition validity over a batch | `epica-zk-evidence/src/zk_skeleton.rs` | **Not implemented.** Ed25519 ships today; RISC Zero is the documented future path. |
| 5.2–6.8 soft contract violations per session (ABC paper baseline) | `docs/contracts.md` | Not reproduced on Epica's runtime. |
| Live Neo4j flush + load smoke test | — | **Not in CI** (TD-P8-004). The `neo4rs 0.8` impl compiles but is not exercised against a real server in our pipeline. |
| `MockLlmClient` parity test from Python | — | TD-P8-008 — Rust unit + integration cover the trait, Python wiring not yet asserted from `pytest`. |

---

## How to audit the evidence

1. **Run all Rust tests**: `cargo test --workspace --exclude epica-python`
2. **Run Python tests**: `cd crates/epica-python && maturin develop && python -m pytest tests/ -v`
3. **Run the MCP smoke**: `EPICA_NO_AUTH=1 cargo run --bin epica-serve` then
   `curl http://localhost:8765/health` + `curl http://localhost:8765/.well-known/epica-server-card.json`
4. **Reproduce the bench**: `target/release/epica-bench run-all --trajectories 200 --out-dir tmp` and diff `tmp/*.csv` against `docs/benchmarks/*.csv` — the CSVs are deterministic byte-for-byte for the same trajectory count.
5. **Audit a sealed ledger end-to-end**:
   ```bash
   epica-verify keygen --secret-out /tmp/sec.hex
   # ... produce some ledger.json ...
   epica-verify seal   --ledger ledger.json --secret /tmp/sec.hex --out receipt.json
   epica-verify verify --ledger ledger.json --receipt receipt.json
   ```

See [`docs/audit_guide.md`](audit_guide.md) for the structured review
path optimised for a hostile reviewer.
