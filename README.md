# Epica

[![CI](https://github.com/angelnicolasc/epica/actions/workflows/ci.yml/badge.svg)](https://github.com/angelnicolasc/epica/actions/workflows/ci.yml)
[![Rust: stable](https://img.shields.io/badge/rust-stable-orange.svg)](https://www.rust-lang.org/)
[![MSRV: 1.82](https://img.shields.io/badge/MSRV-1.82-blue.svg)](https://blog.rust-lang.org/2024/10/17/Rust-1.82.0.html)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

**Epistemic Causal Agent Belief Runtime** - embeddable Rust library for formal causal belief revision in LLM agents.

---

## What Epica does

Epica gives LLM agents a disciplined belief layer: a typed, revision-safe, contract-governed store grounded in five 2026 arXiv papers.

Three concrete capabilities this codebase delivers today:

- **Contradiction-aware belief updates**: when new evidence contradicts an existing belief on the same key, AGM contraction (Alchourron-Gardenfors-Makinson, 1985) applies a Core-Retainment-style minimal contraction over the belief's supporting premises before accepting the new value. Postulates K\*2–K\*5 are enforced as hard errors in all builds; K\*6 is approximated. Cross-belief semantic contradiction (paraphrases, negations) requires explicit graph edges or Phase 4 embedding integration (TD-003).
- **Confidence propagation over causal structure**: System 1 propagates confidence changes through the causal graph via Noisy-OR. System 2 triggers async LLM reflection when confidence diverges from the reliability baseline. T-ECE pipeline validated; real-task calibration benchmarks (ALFWorld/WebShop) not yet measured.
- **Contract-enforced belief mutation**: typed `C = (P, I, G, R)` contracts gate every belief write. Soft violations log and continue. Hard violations trigger recovery. Critical violations halt with a causal diff and escalate.

---

## Why this matters

LLM agents accumulate beliefs across tool calls, user turns, and environment observations. Standard memory layers (vector stores, session state, KV caches) do not:

- detect when a new belief contradicts existing ones
- propagate confidence changes through causal dependencies
- enforce typed policies on who may write, revise, or delete a belief

Epica addresses these gaps with a runtime designed for deployment alongside any LLM framework (Anthropic SDK, LangGraph, MCP-compatible hosts).

---

## What is implemented today

Seven Rust crates compile with `cargo check --workspace --exclude crates/epica-python` (`epica-python` requires Python headers at build time and is verified separately). 29 E2E tests pass in `epica-mcp`. 65 pytest pass in `epica-python` (requires `maturin develop`).

| Capability | Status | Verification path |
|-----------|--------|-------------------|
| BeliefQuad 4-graph store | Implemented | `cargo test -p epica-core` |
| AGM K\*2-K\*5 revision | Implemented | `crates/epica-core/tests/agm_postulates/` |
| AGM K\*6 extensionality | **Approximated** - structural equality only | `tests/agm_postulates/k6_extensionality.rs` |
| System 1 Noisy-OR propagation | Implemented | `tests/integration/system1_propagation.rs` |
| System 2 LLM reflection (sync) | Implemented | `tests/system2_mock.rs`, `crates/epica-anthropic/` |
| T-ECE calibration metric | Implemented; formula pipeline validated | `tests/beliefshift_benchmark.rs` |
| Checkpoint / rollback with K\*4 guard | Implemented | `tests/integration/quad_basic.rs` |
| Behavioral contracts C=(P,I,G,R) | Implemented | `cargo test -p epica-contracts` |
| Mnemonic sovereignty (9 primitives) | Implemented at struct level; enforced in runtime | `crates/epica-contracts/src/sovereignty.rs` |
| Forget-policy verification | Implemented - exhaustive graph traversal | `epica-contracts/src/sovereignty.rs` |
| `#[derive(BeliefState)]` proc macro | Implemented - 9 attributes, all generated methods | `cargo test -p epica-macros` |
| ProspectiveIndex write-time indexing | Implemented with `HashEmbedder` fallback | `crates/epica-runtime/src/prospective/` |
| MCP 2026 server (16 routes) | Implemented | `cargo test -p epica-mcp` (29 E2E) |
| SEP-1686 Tasks primitive | Implemented (tasks complete synchronously) | `tests/e2e_tasks.rs` |
| OAuth 2.1 JWT (HS256 / RS256) | Implemented | `tests/e2e_health.rs` |
| Prometheus metrics | Implemented | `GET /metrics` |
| Python SDK (PyO3) | Implemented | 65 pytest in `crates/epica-python/tests/` |
| Redis persistence | Implemented | `crates/epica-memory/src/redis/` |

---

## What is approximate or pending

| Item | Status | Details |
|------|--------|---------|
| AGM K\*6 | **Approximate** | Structural equality only; semantic equivalence requires embedding comparison (TD-003) |
| Semantic contradiction detection | **Structural only** | Same-ID value comparison (normalized for `Asserted`). Cross-belief semantic contradiction requires explicit `SemanticEdge::Contradicts` or Phase 4 embeddings (TD-003) |
| System 2 async | **Implemented** | LLM reflection spawns a background task; `update_belief()` returns immediately with a `task_id`. Budget refunded on LLM failure. |
| Python System 2 injection | **Not exposed** | `BeliefRuntime::with_llm_client()` not yet bridged to Python (TD-P7-002) |
| Python async (`await`) | **Not available** | `pyo3-asyncio` with pyo3 0.22 is unstable (TD-P6-001) |
| Neo4j persistence | **Returns `Err`** | No stable Rust driver; graceful error present (TD-NEW-001) |
| Phase 1 perf benchmarks | **Targets set; not yet measured** | Run `cargo bench -p epica-core` to measure |

---

## How to verify the claims

```bash
# Compile all seven Rust crates
cargo check --workspace --exclude crates/epica-python

# AGM postulates + integration
cargo test -p epica-core

# BeliefShift benchmark (T-ECE = 0.07 result)
cargo test -p epica-runtime --features system2

# Contract evaluation + drift bounds
cargo test -p epica-contracts

# Proc macro expansion
cargo test -p epica-macros

# MCP server (29 E2E tests via Axum test client)
cargo test -p epica-mcp

# MCP server smoke test
EPICA_NO_AUTH=1 cargo run --bin epica-serve &
curl http://localhost:8765/health
curl http://localhost:8765/.well-known/epica-server-card.json

# Python SDK (requires maturin + Python)
cd crates/epica-python && maturin develop
python -m pytest tests/ -v   # 65 tests
```

See [`docs/audit_guide.md`](docs/audit_guide.md) for a structured review that maps each claim to its implementation and tests.

---

## Crate map

| Crate | Phase | Status | Tests |
|-------|-------|--------|-------|
| `epica-core` | 1 | **Implemented** | AGM postulates + integration suite |
| `epica-runtime` | 2 | **Implemented** | 13 integration + BeliefShift benchmark |
| `epica-contracts` | 3 | **Implemented** | Config, evaluation, drift bounds |
| `epica-macros` | 4 | **Implemented** | 8 unit + trybuild expansion |
| `epica-anthropic` | n/a | **Implemented** | Compiles; live calls require `ANTHROPIC_API_KEY` |
| `epica-mcp` | 5 | **Implemented** | 29 E2E (Axum test client) |
| `epica-python` | 6 | **Implemented** | 65 pytest (requires `maturin develop`) |
| `epica-memory` | 7 | **Partially implemented** | Redis (verified); Neo4j returns `Err` (TD-NEW-001) |

---

## What Epica does that typical memory layers do not

| Feature | Vector store | Graph memory | Session KV | Epica |
|---------|:-----------:|:------------:|:----------:|:-----:|
| Contradiction detection | No | No | No | AGM Core-Retainment contraction |
| Causal confidence propagation | No | No | No | Noisy-OR over CausalGraph |
| Formal revision postulates | No | No | No | K\*2-K\*5 verified; K\*6 approximated |
| Typed contracts on writes | No | No | No | C=(P,I,G,R) |
| Memory governance primitives | No | No | No | 9 primitives (arXiv:2604.16548) |
| MCP 2026 native | Varies | No | No | 16 routes + SEP-1686 Tasks |
| Rollback with AGM guard | No | No | No | K\*4 vacuity enforced |

---

## Current limitations

- **K\*6 is structural**: Epica does not detect semantic equivalence between paraphrased beliefs. Two beliefs with identical meaning but different strings are treated as distinct.
- **System 2 is synchronous**: Under load, LLM reflection blocks the update path. True async requires task-store persistence (TD-P5-002).
- **ProspectiveIndex uses hash embeddings by default**: Without a configured `ProspectiveClient` (e.g., via `epica-anthropic`), write-time indexing uses `HashEmbedder` - a fast offline fallback, not semantic similarity.
- **Causal contradiction is cross-belief only with explicit edges**: `check_contradiction()` detects structural changes on a single belief (same ID, different value). Cross-belief semantic contradiction — paraphrases, negations, synonyms across distinct beliefs — requires explicit `SemanticEdge::Contradicts` edges or Phase 4 embedding integration (TD-003).
- **Causal contradiction is not semantic**: `check_contradiction()` compares JSON values literally. Negations, synonyms, and paraphrases are not caught (TD-003).
- **Python SDK does not expose System 2 LLM injection**: `BeliefRuntime::with_llm_client()` is not bridged to Python; System 2 always returns `System1Only` or `System2Throttled` from Python (TD-P7-002).

See [`docs/non_goals.md`](docs/non_goals.md) for accepted tradeoffs and [`docs/evidence.md`](docs/evidence.md) for the full evidence inventory.

---

## Theoretical foundation

Five papers from Q1-Q2 2026 ground the architecture:

| Paper | Insight implemented |
|-------|---------------------|
| MAGMA (arXiv:2601.03236) | Four orthogonal graphs instead of one monolithic graph |
| Kumiho (arXiv:2603.17244) | Property graph operations correspond to AGM postulates; prospective indexing |
| Agentic UQ (arXiv:2601.15703) | Dual-process uncertainty as control (not sensor); Trajectory-ECE metric |
| Agent Behavioral Contracts (arXiv:2602.22302) | Formal `C=(P,I,G,R)` contracts with (p,delta,k)-satisfaction bounds |
| Mnemonic Sovereignty (arXiv:2604.16548) | Nine memory governance primitives as enforcement invariants |

---

## Quick start

```rust
use epica_core::{BeliefQuad, BeliefNode, BeliefValue, Provenance};

let mut quad = BeliefQuad::new();

let node = BeliefNode::new(
    "user_intent",
    BeliefValue::Inferred(serde_json::json!("refactor auth module")),
    Provenance::LlmInference {
        model: "claude-sonnet-4-6".into(),
        call_id: uuid::Uuid::new_v4(),
        prompt_hash: 0,
    },
    0.85,
);
let id = quad.insert(node);

// AGM revision: detects contradiction, applies Core-Retainment contraction, then expands
quad.revise(
    id,
    BeliefValue::Inferred(serde_json::json!("refactor auth + session modules")),
    Provenance::LlmInference {
        model: "claude-sonnet-4-6".into(),
        call_id: uuid::Uuid::new_v4(),
        prompt_hash: 1,
    },
    0.90,
).unwrap();

// System 1 propagates confidence changes to causal descendants after revise()
let checkpoint_id = quad.checkpoint();
let diff = quad.diff(&BeliefQuad::new());
```

For a full walkthrough from belief insertion through contradiction, rollback, and contract enforcement, see [`docs/end_to_end_example.md`](docs/end_to_end_example.md).

---

## Benchmarks

| Benchmark | Target | Current result | How measured | Gap |
|-----------|--------|----------------|--------------|-----|
| BeliefShift T-ECE (formula validation) | < 0.08 | **0.07 (pipeline only)** | `tests/beliefshift_benchmark.rs` — `pipeline_tece_formula_validation` confirms formula computes correctly; `beliefshift_tece_variable_confidence` uses varied confidences | Real-task calibration (ALFWorld/WebShop) not yet measured |
| System 1 propagation at 10K nodes | < 2x HashMap | Not yet measured | `cargo bench -p epica-core` | Pending |
| Checkpoint + rollback at 10K nodes | < 10ms | Not yet measured | `cargo bench -p epica-core` | Pending |

---

## Documentation

| Document | Covers |
|----------|--------|
| [`docs/architecture.md`](docs/architecture.md) | Why four graphs; invariants; data flow; design tradeoffs |
| [`docs/agm_postulates.md`](docs/agm_postulates.md) | Postulate-by-postulate: exact vs. approximate compliance |
| [`docs/dual_process.md`](docs/dual_process.md) | System 1 and System 2 mechanics; T-ECE benchmark result |
| [`docs/contracts.md`](docs/contracts.md) | Contract components; runtime enforcement points; violation classes |
| [`docs/mnemonic_sovereignty.md`](docs/mnemonic_sovereignty.md) | Nine governance primitives; what "verifiable deletion" means here |
| [`docs/mcp_server.md`](docs/mcp_server.md) | Endpoint table with per-route implementation status |
| [`docs/phase_roadmap.md`](docs/phase_roadmap.md) | Per-phase status; verification commands; known gaps |
| [`docs/evidence.md`](docs/evidence.md) | Test inventory; benchmark results; what has been verified |
| [`docs/non_goals.md`](docs/non_goals.md) | What Epica does not attempt; accepted tradeoffs |
| [`docs/audit_guide.md`](docs/audit_guide.md) | Structured review path for a hostile technical audience |
| [`docs/end_to_end_example.md`](docs/end_to_end_example.md) | Narrative walkthrough: contradiction -> AGM -> rollback -> contract |
| [`docs/competitive_landscape.md`](docs/competitive_landscape.md) | Honest comparison against alternatives |

---

## Contributing

`cargo check --workspace --exclude crates/epica-python` must pass. `cargo clippy --workspace --exclude crates/epica-python` must pass with zero warnings.

See [`docs/architecture.md`](docs/architecture.md) for the invariants you must preserve.

---

## License

MIT
