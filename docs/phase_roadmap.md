# Phase Roadmap

> This document is the operational state of the project. It describes what is implemented, what is verified, and what gaps remain per phase. For claimed capabilities without a verification path listed, treat the claim as unverified until tested.

## Status legend

| Symbol | Meaning |
|--------|---------|
| **Implemented** | Compiled, tested, no `todo!()` stubs |
| **Implemented (partial)** | Compiled, tested, but known functional gaps |
| **Approximate** | Logic present but formally limited (see gap column) |
| **Returns `Err`** | Graceful error; feature not available |

---

## Phase status

| Phase | Crate | Status | What is implemented | Known gaps |
|-------|-------|--------|---------------------|------------|
| 1 | `epica-core` | **Implemented** | BeliefQuad, 4 graphs, AGM K\*2-K\*5, System 1, checkpoint/rollback, diff, T-ECE, counterfactual, `#[serde]` serialization, PageRank centrality | K\*6 approximate (TD-003); `ProspectiveIndex.index_belief()` was no-op until Phase 4 |
| 2 | `epica-runtime` | **Implemented** | `BeliefRuntime`, dual-process System 1+2, `TokenBucket` with real refill (default: 50 reflections/session, `refill_rate` = 1.0/s), `AnthropicLlmClient` (via `epica-anthropic`), `ConfidenceHistory`, `SessionReport`, multicriteria retrieval, T-ECE computation | System 2 calls are synchronous (TD-P5-002); `LlmClient` optional - degrades to `System1Only` without it |
| 3 | `epica-contracts` | **Implemented** | `BehavioralContract` C=(P,I,G,R), `ContractEngine`, `GovernanceTracker`, all 9 Mnemonic Sovereignty primitives, drift-bound computation via CLT, TOML-deserializable `ContractConfig` | - |
| 4 | `epica-macros` | **Implemented** | `#[derive(BeliefState)]` with 9 attributes, typed accessors, `default_contract()`, `schema_descriptor()`, `to/from_belief_quad()`; `ProspectiveIndex` write-time LLM wiring (TD-001 resolved); `HashEmbedder`; `ProspectiveClient` + `Embedder` traits | `causal_events` field of `RawProspectiveScenario` not populated (Phase 5 work) |
| - | `epica-anthropic` | **Implemented** | `AnthropicLlmClient` implementing `LlmClient` + `ProspectiveClient` traits; `claude-sonnet-4-6` for System 2; `claude-haiku-4-5-20251001` for prospective indexing | Live calls require `ANTHROPIC_API_KEY`; no retry logic |
| 5 | `epica-mcp` | **Implemented** | Full Axum MCP 2026 server, 16 routes, SEP-1686 Tasks primitive, OAuth 2.1 JWT (HS256 + RS256), per-IP rate limiting (`governor`), Prometheus metrics, Server Card at `/.well-known/epica-server-card.json`, SSE streaming for tasks | Tasks complete synchronously (System 2 is sync); task store is in-memory (TD-P5-002) |
| 6 | `epica-python` | **Implemented** | Full PyO3 SDK: `PyBeliefQuad` (all CRUD, AGM, checkpoint, counterfactual, 4 edge types, dict protocol), `PyBeliefRuntime`, `PyBehavioralContract`, decorators (`@belief_state`, `@governed_by`), integrations (`AnthropicBeliefSession`, `EpicaBeliefTool` for LangChain), PEP 561 `.pyi` stubs | System 2 LLM not exposed to Python (TD-P7-002); no async bridge (TD-P6-001); not in `default-members` (TD-P7-001) |
| 7 | `epica-memory` | **Implemented (partial)** | `LongTermMemoryStore` trait, `FlushResult`, `SchemaDescriptor`; Redis backend fully implemented with sovereignty-aware TTL | Neo4j backend returns `Err("Neo4j driver not yet available")` (TD-NEW-001) |

---

## Verification per phase

### Phase 1 - verified

```bash
cargo check -p epica-core
cargo test  -p epica-core        # AGM postulates + integration tests
cargo clippy -p epica-core
```

Verified outputs:
- All AGM postulate tests pass (`k2_success`, `k3_inclusion`, `k4_vacuity`, `k5_consistency`, `k6_extensionality`)
- `system1_propagation` integration test passes
- `quad_basic` integration test passes (insert, remove, checkpoint, rollback)

### Phase 2 - verified

```bash
cargo check -p epica-runtime --features system2
cargo test  -p epica-runtime --features system2
```

Verified outputs:
- `system1_only.rs` - 5 tests (CRUD, rollback, retrieval ordering, TTL expiry)
- `system2_mock.rs` - 4 tests (activation, no-activation, throttle, slow_confidence)
- `tece_session.rs` - 4 tests (history ground-truth, finalize_session, System 2 confidence)
- `beliefshift_benchmark.rs` - T-ECE = **0.07 < 0.08 (verified)** (deterministic 25-step session)

### Phase 3 - verified

```bash
cargo check -p epica-contracts
cargo test  -p epica-contracts
```

Verified outputs:
- 7+ tests in `config.rs` (TOML deserialization, contract evaluation, drift bound)
- Precondition, invariant, and governance evaluation tested

### Phase 4 - verified

```bash
cargo check -p epica-macros
cargo test  -p epica-macros
cargo check -p epica-anthropic
```

Verified outputs:
- 8 unit tests (insert count, round-trip, confidence accessors, TTL, prospect flag)
- trybuild expansion tests (`basic.rs`, `full_attrs.rs`)

### Phase 5 - verified

```bash
cargo check -p epica-mcp
cargo test  -p epica-mcp     # 29 E2E tests via Axum test client

# Manual smoke test:
EPICA_NO_AUTH=1 cargo run --bin epica-serve
curl http://localhost:8765/health
curl http://localhost:8765/.well-known/epica-server-card.json | python -m json.tool
curl -X POST http://localhost:8765/v1/beliefs \
     -H 'Content-Type: application/json' \
     -d '{"key":"user_intent","value":"refactor auth","confidence":0.9}'
curl http://localhost:8765/metrics
```

Verified outputs:
- `e2e_belief_lifecycle.rs` - 8 tests
- `e2e_checkpoint_rollback.rs` - 5 tests
- `e2e_health.rs` - 6 tests (health, ready, server card, JWKS, metrics)
- `e2e_query.rs` - 6 tests
- `e2e_tasks.rs` - 4 tests

### Phase 6 - verified (requires Python environment)

```bash
cd crates/epica-python
maturin develop
python -m pytest tests/ -v   # 65 tests
```

Verified outputs:
- `test_belief_quad.py` - 22 tests
- `test_runtime.py` - 14 tests
- `test_contracts.py` - 14 tests
- `test_decorators.py` - 9 tests
- `test_e2e.py` - 6 tests

### Phase 7 - partial

```bash
cargo check -p epica-memory
# Redis integration test requires a running Redis instance:
cargo test  -p epica-memory --features redis
```

Neo4j: `Neo4jMemoryStore::connect()` returns `Err(MemoryError::Connection(...))`. No test exercises the Neo4j path successfully.

---

## Open technical debts

| ID | Crate | Description | Impact |
|----|-------|-------------|--------|
| TD-003 | `epica-core` | Semantic contradiction detection via embeddings | K\*6 remains approximate; paraphrases not caught |
| TD-P5-002 | `epica-mcp` | Task store persistence (in-memory only) | Tasks lost on server restart; System 2 appears sync |
| TD-P6-001 | `epica-python` | `pyo3-asyncio` async bridge | No native `await` from Python async functions |
| TD-P6-003 | `epica-python` | Maturin auto-generated stubs | Manual `.pyi` stubs may drift from implementation |
| TD-P7-001 | workspace | `epica-python` not in `default-members` | `cargo check --workspace` does not verify the Python crate |
| TD-P7-002 | `epica-python` | `BeliefRuntime::with_llm_client()` not exposed | System 2 LLM inaccessible from Python |
| TD-NEW-001 | `epica-memory` | Neo4j driver not available | Causal graph cross-session persistence not implemented |
