# Audit Guide

This document gives a skeptical technical reviewer the shortest path to verify the central claims of this repository. It maps claims to code, tests, and known limitations.

---

## Recommended reading order

1. `README.md` (project root) - capabilities, crate map, limitations table
2. `docs/architecture.md` - why four graphs, invariants, tradeoffs
3. `docs/agm_postulates.md` - exactly what "AGM compliance" means here (K\*2-K\*5 exact; K\*6 approximate)
4. `docs/evidence.md` - test inventory and benchmark results
5. `docs/non_goals.md` - what is deliberately out of scope
6. `docs/phase_roadmap.md` - per-phase implementation status with verification commands

Then drill into whichever claim you want to audit:
- Contracts -> `docs/contracts.md` -> `crates/epica-contracts/`
- Uncertainty -> `docs/dual_process.md` -> `crates/epica-runtime/tests/`
- Governance -> `docs/mnemonic_sovereignty.md` -> `crates/epica-contracts/src/sovereignty.rs`
- MCP server -> `docs/mcp_server.md` -> `crates/epica-mcp/tests/`

---

## Claim-by-claim verification

### Claim: AGM K\*2-K\*5 are satisfied

**Where to verify**: `crates/epica-core/tests/agm_postulates/`

Each postulate has a dedicated test file: `k2_success.rs`, `k3_inclusion.rs`, `k4_vacuity.rs`, `k5_consistency.rs`, `k6_extensionality.rs`.

```bash
cargo test -p epica-core 2>&1 | grep agm
```

**Known limitation**: K\*6 is structural equality only. Two semantically equivalent beliefs with different string representations will not be treated as equivalent. This is documented and tracked as TD-003.

**PostulateAudit caveat**: in release builds, postulate violations are recorded but do not block the mutation. Audit the `PostulateAudit` struct in `RevisionRecord` if you want to confirm violations are captured.

---

### Claim: System 1 propagates confidence through the causal graph

**Where to verify**: `crates/epica-core/tests/integration/system1_propagation.rs`

```bash
cargo test -p epica-core integration::system1_propagation
```

Inspect `crates/epica-core/src/system1/mod.rs` for the Noisy-OR formula and cycle guard.

**Known limitation**: Epica's System 1 is external-runtime propagation, not internal Transformer attention propagation. These are different quantities. The approximation rationale is in `docs/dual_process.md`.

---

### Claim: T-ECE = 0.07 < 0.08 target

**Where to verify**: `crates/epica-runtime/tests/beliefshift_benchmark.rs`

```bash
cargo test -p epica-runtime --features system2 beliefshift_benchmark
```

Read the test directly - the scenario is deterministic by design (22 correct at 0.93 confidence + 3 incorrect at 0.07). It validates the T-ECE computation pipeline, not real-world calibration.

**Known limitation**: this is a deterministic benchmark, not a measurement on a real partial-observability task. Real-world T-ECE performance is not yet measured.

---

### Claim: Behavioral contracts C=(P,I,G,R) enforce on every belief write

**Where to verify**: `crates/epica-contracts/`, `crates/epica-runtime/src/runtime.rs`

Look at `update_belief()` in `runtime.rs` - find the `check_preconditions()` call before the revision and `check_invariants()` call after.

```bash
cargo test -p epica-contracts
```

**Known limitation**: `(p, delta, k)`-satisfaction bounds are computed analytically from CLT; they have not been measured against a real agent workload. Violation counts from the ABC paper have not been reproduced.

---

### Claim: MCP 2026 server with 16 routes, SEP-1686 Tasks

**Where to verify**: `crates/epica-mcp/tests/`

```bash
cargo test -p epica-mcp

# Manual smoke test:
EPICA_NO_AUTH=1 cargo run --bin epica-serve &
curl http://localhost:8765/health
curl http://localhost:8765/.well-known/epica-server-card.json
curl -X POST http://localhost:8765/v1/beliefs \
     -H 'Content-Type: application/json' \
     -d '{"key":"test","value":"hello","confidence":0.9}'
curl http://localhost:8765/v1/beliefs/test
```

**Known limitation**: SEP-1686 tasks are synchronously completed - the "pending" state is never observed because System 2 is currently synchronous. The task polling endpoint works correctly but always returns an already-completed task.

---

### Claim: Nine Mnemonic Sovereignty primitives

**Where to verify**: `crates/epica-contracts/src/sovereignty.rs`

Count the struct fields in `MnemonicSovereignty`: `write_auth`, `read_auth`, `update_auth`, `retention`, `forget`, `audit`, `cross_agent`, `rollback_auth`, `recovery` - nine primitives.

Inspect `ForgetPolicy.verify_fn` - it traverses all four graphs after erasure and returns `Err` if the belief is still reachable.

**Known limitation**: the `verify_fn` is a graph traversal, not a cryptographic erasure proof. It confirms in-process erasure; it does not cover Redis, disk snapshots, or cross-agent copies.

---

### Claim: Python SDK with complete System 1 API

**Where to verify**: `crates/epica-python/tests/`

```bash
cd crates/epica-python && maturin develop
python -m pytest tests/ -v
```

**Known limitation**: System 2 LLM injection is not exposed to Python (TD-P7-002). All `update_belief()` calls from Python operate in `System1Only` mode unless System 2 fires on the Rust side without a client. No async `await` bridge (TD-P6-001).

---

## What is conceptual vs. truth surface

### Conceptual documents (design rationale, paper mapping)

- `docs/architecture.md` - explains design choices; implementation pointers included
- `docs/dual_process.md` - explains paper-to-runtime mapping; divergences documented
- `docs/contracts.md` - explains `C = (P, I, G, R)` and enforcement flow; links to runtime

### Truth surface (operational state)

- `docs/evidence.md` - exact test counts, benchmark results, unverified claims
- `docs/phase_roadmap.md` - per-phase implementation status with verification commands
- `docs/mcp_server.md` - per-endpoint status table
- `docs/agm_postulates.md` - per-postulate exact vs. approximate table

---

## What is experimental vs. stable

### Stable core (Phase 1-3)

- `epica-core`: BeliefQuad, AGM revision, System 1 - fully tested, no known breaking changes planned
- `epica-contracts`: BehavioralContract, MnemonicSovereignty - implemented and tested
- `epica-macros`: `#[derive(BeliefState)]` - implemented with trybuild tests

### Stable but with known gaps (Phase 4-6)

- `epica-anthropic`: implemented; no retry logic; live calls depend on `ANTHROPIC_API_KEY`
- `epica-mcp`: full implementation; task async behavior is degenerate (always `Completed`); task store is in-memory
- `epica-python`: full API; System 2 not exposed; no async bridge

### Partial (Phase 7)

- `epica-memory`: Redis works; Neo4j returns `Err`

---

## Anticipated objections

**"K\*6 is not satisfied"** - Correct. The approximation is documented in `docs/agm_postulates.md` with the specific condition under which it fails. TD-003 tracks the fix.

**"System 2 is not really async"** - Correct. Tasks appear immediately as `Completed`. The SEP-1686 structural scaffolding is correct; the sync limitation is documented as TD-P5-002.

**"The benchmark is deterministic, not real-world"** - Correct. The BeliefShift benchmark validates the T-ECE computation pipeline. Real-world calibration is not yet measured.

**"PostulateAudit doesn't block mutations in production"** - Correct by design. The reasoning is in `docs/architecture.md` (audit trail vs. gate). Contracts provide hard enforcement.

**"ProspectiveIndex uses hash embeddings"** - Correct when no `ProspectiveClient` is configured. The `HashEmbedder` is an offline fallback, not semantic similarity. Using `epica-anthropic` wires a real LLM embedding.

**"Neo4j is not implemented"** - Correct. `Neo4jMemoryStore` returns `Err`. Redis covers the working-memory use case.
