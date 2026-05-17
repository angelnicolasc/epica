# Evidence

This document inventories what has been verified in this codebase, how to reproduce each result, and which claims have no current empirical backing.

---

## Test inventory

### epica-core

| Test suite | File | Tests | What is verified |
|-----------|------|-------|-----------------|
| AGM postulates | `crates/epica-core/tests/agm_postulates/` | 5 files | K\*2 success, K\*3 inclusion, K\*4 vacuity, K\*5 consistency, K\*6 extensionality (structural) |
| Integration | `crates/epica-core/tests/integration/` | 2 files | Basic quad CRUD + rollback; System 1 Noisy-OR propagation |

```bash
cargo test -p epica-core
```

### epica-runtime

| Test suite | File | Tests | What is verified |
|-----------|------|-------|-----------------|
| System 1 only | `tests/system1_only.rs` | 5 | CRUD, rollback, retrieval ordering, TTL expiry - no LLM |
| System 2 mock | `tests/system2_mock.rs` | 4 | System 2 activation, no-activation, throttle, slow_confidence update |
| T-ECE session | `tests/tece_session.rs` | 4 | History ground truth, `finalize_session()`, System 2 confidence recording |
| BeliefShift | `tests/beliefshift_benchmark.rs` | 1 | T-ECE = 0.07 < 0.08 (deterministic 25-step session) |

```bash
cargo test -p epica-runtime --features system2
```

### epica-contracts

| Test suite | File | Tests | What is verified |
|-----------|------|-------|-----------------|
| Config | `src/config.rs` | 7 | TOML deserialization, `from_config()`, precondition evaluation, invariant evaluation, drift bound, min-confidence default |
| (additional contract tests) | `tests/` | n/a | Precondition gating, hard violation, critical halt |

```bash
cargo test -p epica-contracts
```

### epica-macros

| Test suite | File | Tests | What is verified |
|-----------|------|-------|-----------------|
| Unit | `tests/belief_state_tests.rs` | 8 | Insert count, round-trip, confidence defaults, typed/generic accessors, unknown key, TTL parsing, prospect flag, reflection threshold |
| Trybuild | `tests/trybuild.rs` | 2 | Proc macro expansion compiles without errors (`basic.rs`, `full_attrs.rs`) |

```bash
cargo test -p epica-macros
```

### epica-mcp

| Test suite | File | Tests | What is verified |
|-----------|------|-------|-----------------|
| Belief lifecycle | `tests/e2e_belief_lifecycle.rs` | 8 | Insert, get, update, round-trip, 404, provenance, multiple beliefs, readiness count |
| Checkpoint/rollback | `tests/e2e_checkpoint_rollback.rs` | 5 | Checkpoint, rollback, invalid rollback, diff with T-ECE, unknown checkpoint |
| Health | `tests/e2e_health.rs` | 6 | Liveness, readiness, server card fields, server card endpoints, JWKS, Prometheus |
| Query | `tests/e2e_query.rs` | 6 | Empty query, query post-inserts, default budget, counterfactual 404, counterfactual surviving, contract status |
| Tasks | `tests/e2e_tasks.rs` | 4 | Task 404, task without System 2, task poll structure, task SSE reachable |

```bash
cargo test -p epica-mcp
```

### epica-python (requires Python + maturin)

| Test suite | File | Tests | What is verified |
|-----------|------|-------|-----------------|
| BeliefQuad | `tests/test_belief_quad.py` | 22 | CRUD, AGM revision, checkpoints, counterfactual, graph edges, dict protocol, dunders |
| Runtime | `tests/test_runtime.py` | 14 | Insert/get/update, retrieve, context manager, session report, T-ECE |
| Contracts | `tests/test_contracts.py` | 14 | Preconditions (pass/fail/raise), invariants, severity levels, multiple constraints |
| Decorators | `tests/test_decorators.py` | 9 | `@belief_state` with/without contract; `@governed_by` (pass/fail/return value/without belief_state) |
| E2E | `tests/test_e2e.py` | 6 | Full lifecycle, contract-governed quad, T-ECE bounded, decorator pipeline, counterfactual with causal chain, retrieve ordering |

```bash
cd crates/epica-python && maturin develop
python -m pytest tests/ -v
```

---

## Benchmark results

| Benchmark | Target | Result | Command | Notes |
|-----------|--------|--------|---------|-------|
| BeliefShift T-ECE | < 0.08 | **0.07 (verified)** | `cargo test -p epica-runtime --features system2 beliefshift_benchmark` | Deterministic; 25 steps |
| System 1 propagation at 10K nodes | < 2x HashMap | Not yet measured | `cargo bench -p epica-core` | Benchmark harness exists; no recorded result |
| Checkpoint + rollback at 10K nodes | < 10ms | Not yet measured | `cargo bench -p epica-core` | Benchmark harness exists; no recorded result |

---

## Claims with no current empirical backing

The following claims appear in the codebase or documentation but have not been measured against real workloads:

| Claim | Location | Current status |
|-------|----------|---------------|
| T-ECE < 0.08 on real partial-observability tasks | `docs/dual_process.md` | Validated on deterministic benchmark only; ALFWorld/WebShop not tested |
| 5.2-6.8 soft contract violations per session | `docs/contracts.md` | ABC paper baseline; not reproduced on Epica's runtime |
| System 1 propagation < 2x HashMap at 10K nodes | `README.md` benchmarks | Target defined; not yet measured |
| Checkpoint + rollback < 10ms at 10K nodes | `README.md` benchmarks | Target defined; not yet measured |
| ProspectiveIndex retrieval improvement | `docs/phase_roadmap.md` | `HashEmbedder` fallback active by default; semantic retrieval not benchmarked |

---

## How to audit the evidence

1. **Run all Rust tests**: `cargo test --workspace --exclude crates/epica-python`
2. **Run Python tests**: `cd crates/epica-python && maturin develop && python -m pytest tests/ -v`
3. **Run the MCP smoke test**: `EPICA_NO_AUTH=1 cargo run --bin epica-serve` then `curl http://localhost:8765/health`
4. **Inspect test files** directly - no mocking of the core Rust logic (System 2 is mocked in `system2_mock.rs` via `MockLlmClient` defined inline)

See [`docs/audit_guide.md`](audit_guide.md) for a structured review path.
