# Audit Guide

This document gives a skeptical technical reviewer the shortest path to verify the central claims of this repository. It maps claims to code, tests, and known limitations. Updated for the post-public-review hardening (Sprints 1–4).

---

## Recommended reading order

1. `README.md` (project root) — capabilities, crate map, limitations table
2. `docs/architecture.md` — why four graphs, invariants, design tradeoffs, data flow
3. `docs/agm_postulates.md` — exactly what "AGM compliance" means here (K\*2–K\*6 real)
4. `docs/evidence.md` — full test inventory and benchmark results
5. `docs/non_goals.md` — what is deliberately out of scope
6. `docs/phase_roadmap.md` — per-phase / per-sprint status with verification commands

Then drill into whichever claim you want to audit:

- Contracts → `docs/contracts.md` → `crates/epica-contracts/`
- Uncertainty → `docs/dual_process.md` → `crates/epica-runtime/tests/`
- Governance → `docs/mnemonic_sovereignty.md` → `crates/epica-contracts/src/sovereignty/`
- MCP server → `docs/mcp_server.md` → `crates/epica-mcp/tests/`
- Active Inference → `crates/epica-active-inference/src/lib.rs` → `crates/epica-runtime/tests/active_inference_hook.rs`
- Audit ledger → `crates/epica-contracts/src/sovereignty/ledger.rs` → `crates/epica-contracts/tests/audit_ledger.rs`
- Evidence receipts → `crates/epica-zk-evidence/` → `crates/epica-zk-evidence/tests/cli_smoke.rs`
- Benchmarks → `crates/epica-benchmarks/` → `docs/benchmarks/`

---

## Claim-by-claim verification

### Claim: AGM K\*2–K\*6 are enforced as hard errors

**Where to verify**: `crates/epica-core/tests/agm_postulates/`

Each postulate has a dedicated test file: `k2_success.rs`, `k3_inclusion.rs`,
`k4_vacuity.rs`, `k5_consistency.rs`, `k6_extensionality.rs`.

```bash
cargo test -p epica-core --test agm_postulates
```

The gate lives in
[`crates/epica-core/src/revision/agm.rs`](../crates/epica-core/src/revision/agm.rs):

```rust
let audit = PostulateAudit::verify(self, belief_id, &new_value, contradicts, trace);
if !audit.all_critical_pass() {
    return Err(BeliefRevisionError::PostulateViolation {
        postulate: audit.failed_postulate_name(),
    });
}
```

K\*2, K\*3, K\*5, and K\*6 are critical — a violation **rejects the
revision**. K\*4 (vacuity) is informational only: `vacuity = false` means
a contraction was needed, which is a legitimate outcome, not an error.

**No longer a limitation**: earlier versions recorded postulate violations
without blocking the mutation. This has changed — K\*2/K\*3/K\*5/K\*6
are hard errors in every build (confirmed by the `k{2,3,5,6}_*.rs` proptest
suites that verify rejection behaviour).

---

### Claim: K\*6 detects semantic paraphrases (not just structural equality)

**Where to verify**:

```bash
# 4 cases: vacuous/no-provider, paraphrase-not-contradiction,
# anti-parallel-is-contradiction, witness detection
cargo test -p epica-core --test agm_postulates k6

# End-to-end against the real OpenAI-compatible provider (wiremock socket)
cargo test -p epica-openai --test embeddings
# Includes: k6_semantic_paraphrase_works_against_warmed_provider
```

The implementation lives in
[`crates/epica-core/src/embedding/mod.rs`](../crates/epica-core/src/embedding/mod.rs)
(`EmbeddingProvider` trait, `NullEmbeddingProvider`, `CachedEmbeddingProvider`)
and [`crates/epica-core/src/quad/semantic.rs`](../crates/epica-core/src/quad/semantic.rs)
(`value_contradicts_semantic`, `VerdictTrace`).

When `BeliefQuad::set_embedding_provider(provider)` is called:

1. `value_contradicts_semantic` checks the cache for `Asserted/Asserted` pairs.
2. Cosine similarity classifies into `Equivalent` (≥ 0.92), `Contradicts`
   (≤ −0.30), or `Undecided`. Cache miss → literal fallback (same as
   pre-Sprint-1).
3. An `extensionality_witness` in `PostulateAudit` identifies the
   paraphrase peer if K\*6 would be violated — violations are diagnosable,
   not silent.

**Known limitation**: the semantic path applies to `Asserted/Asserted` only.
`Inferred(JsonValue)` and `Deterministic(JsonValue)` use structural equality
— documented as TD-P8-003.

---

### Claim: System 1 propagates confidence through the causal graph

**Where to verify**: `crates/epica-core/tests/integration.rs::system1_propagation`

```bash
cargo test -p epica-core integration::system1_propagation
```

Inspect `crates/epica-core/src/system1/mod.rs` for the Noisy-OR formula
and the cycle guard (`HashSet<BeliefId>` visited set — non-optional, as a
cycle without it causes infinite recursion).

**Known limitation**: Epica's System 1 is external-runtime propagation over
an agent-maintained causal graph, not internal Transformer attention
propagation. These are different quantities. The approximation rationale is
in `docs/dual_process.md`.

---

### Claim: T-ECE = 0.07 < 0.08 calibration target

**Where to verify**: `crates/epica-runtime/tests/beliefshift_benchmark.rs`

```bash
cargo test -p epica-runtime --features system2 beliefshift_benchmark
```

The scenario is deterministic (22 correct at 0.93 confidence + 3 incorrect
at 0.07). It validates the T-ECE computation pipeline, not real-world
calibration.

**For real-workload T-ECE**: see the `epica-bench` harness results below.

---

### Claim: Behavioral contracts C=(P,I,G,R) enforce on every belief write

**Where to verify**: `crates/epica-contracts/`, `crates/epica-runtime/src/runtime.rs`

`update_belief()` in `runtime.rs` calls `check_preconditions()` before
the revision and `check_invariants()` after.

```bash
cargo test -p epica-contracts
cargo test -p epica-runtime --test contracts
```

**Known limitation**: `(p, δ, k)`-satisfaction bounds are computed
analytically from CLT; they have not been measured against a real agent
workload. Violation counts from the ABC paper have not been reproduced on
Epica's runtime (documented in `docs/evidence.md`).

---

### Claim: Tamper-evident BLAKE3 Merkle audit ledger

**Where to verify**: `crates/epica-contracts/src/sovereignty/ledger.rs`,
`crates/epica-contracts/tests/audit_ledger.rs`

```bash
cargo test -p epica-contracts --test audit_ledger
```

Eight integration tests cover: `emit()` seals each entry before
dispatching; shared ledger aggregation; tamper-detection (entry hash
mismatch and chain link broken); a sophisticated-forger scenario where
downstream links are repaired — the Merkle root still diverges; and
`merkle_proof(seq)` + `verify_merkle_proof()` round-trip.

Key property: `self_hash = BLAKE3(prev_hash ‖ canonical_json(entry))`.
Modifying entry N means entry N has a wrong `self_hash`, which means
entry N+1's `prev_hash` disagrees with N's `self_hash`, and so on for
the whole downstream chain. `verify_chain()` catches this in O(N).

---

### Claim: Ed25519 EvidenceReceipt + `epica-verify` CLI for offline verification

**Where to verify**: `crates/epica-zk-evidence/`

```bash
# 23 tests: 18 unit + 4 CLI smoke + 1 doctest
cargo test -p epica-zk-evidence

# Manual E2E:
target/debug/epica-verify keygen --secret-out /tmp/sec.hex
# ... produce ledger.json from a runtime session ...
target/debug/epica-verify seal   --ledger ledger.json --secret /tmp/sec.hex --out receipt.json
target/debug/epica-verify verify --ledger ledger.json --receipt receipt.json
# Prints: OK: receipt verifies — entries 0..=N sealed by <pubkey>
```

The verifier applies four rejection layers in order: (1) receipt
length/schema, (2) range + Merkle root match, (3) Ed25519 signature,
(4) per-entry Merkle inclusion proofs. Each layer has a dedicated
rejection test in `src/verifier.rs`.

**Honest scope**: RISC Zero ZK proofs of AGM transition validity are
deferred (feature `risc0`, skeleton in `zk_skeleton.rs`). Ed25519 over
the Merkle root delivers non-repudiation + tamper evidence + offline
verifiability — which is what an enterprise auditor needs. ZK adds
*privacy*, not *correctness*. See `DEVLOG.md § Sprint 3.2` for the
cost/value reasoning.

---

### Claim: Active Inference / Free Energy monitor detects whole-agent drift

**Where to verify**: `crates/epica-active-inference/`, `crates/epica-runtime/tests/active_inference_hook.rs`

```bash
cargo test -p epica-active-inference          # 16 unit tests
cargo test -p epica-runtime --features active-inference \
           --test active_inference_hook       # 5 integration tests
# Includes: insert_without_monitor_is_zero_cost_default
# (pins that the feature-off path has zero runtime cost)
```

The Friston VFE is computed as:
`F = Σ_i KL(q_i ‖ p_i) − E_q[ln p(o_last ‖ s_last)]`

where `q_i = fast_confidence` (posterior) and `p_i` = NoisyOr of causal
parents (prior). The monitor lives behind `--features active-inference`
on `epica-runtime`; with the feature on, every `insert_belief` calls
`observe(&quad, &node)` and emits a `SurpriseSignal`.

**Honest mapping**: the monitor maps over the `BeliefQuad`, not LLM
internals. The Friston paper is substrate-agnostic; the mapping is
documented in `crates/epica-active-inference/src/lib.rs`.

**The hook never blocks the mutation** — failures (mutex poison, race)
are logged. Hard enforcement is a `BehavioralContract` invariant's job.

---

### Claim: MCP 2026 server with 16 routes, SEP-1686 Tasks

**Where to verify**: `crates/epica-mcp/tests/`

```bash
cargo test -p epica-mcp     # 29 E2E tests

# Manual smoke test:
EPICA_NO_AUTH=1 cargo run --bin epica-serve &
curl http://localhost:8765/health
curl http://localhost:8765/.well-known/epica-server-card.json
curl -X POST http://localhost:8765/v1/beliefs \
     -H 'Content-Type: application/json' \
     -d '{"key":"test","value":"hello","confidence":0.9}'
curl http://localhost:8765/v1/beliefs/test
```

SEP-1686 Tasks return real `Pending` state when System 2 is active
(verified in `e2e_tasks.rs`). Task store uses `SledTaskStore` (feature
`sled-store`, closed TD-P5-002) for persistence across restarts.

---

### Claim: Nine Mnemonic Sovereignty primitives

**Where to verify**: `crates/epica-contracts/src/sovereignty/mod.rs`

Nine fields in `MnemonicSovereignty`: `write_auth`, `read_auth`,
`update_auth`, `retention`, `forget`, `audit`, `cross_agent`,
`rollback_auth`, `recovery`.

```bash
cargo test -p epica-contracts --test sovereignty   # 21 tests
```

`ForgetPolicy::verify_fn` traverses all four graphs after erasure and
returns `Err` if the belief is still reachable via any graph edge.

**Known limitation**: `verify_fn` confirms in-process erasure; it does
not cover Redis persistence, disk snapshots, or cross-agent copies.

---

### Claim: Python SDK with `LlmClient` injection

**Where to verify**: `crates/epica-python/tests/`

```bash
cd crates/epica-python && maturin develop
python -m pytest tests/ -v   # 65 tests
```

`PyMockLlmClient.handle()` produces a `PyLlmClientHandle` that can be
passed to `BeliefRuntime::attach_llm_client()` from Python, enabling
System 2 reflection driven entirely from Python (resolved TD-P7-002).

**Known limitation**: native `await` bridge needs `pyo3-asyncio` 0.22
(TD-P6-001). `MockLlmClient` parity from `pytest` is TD-P8-008.

---

### Claim: Real Neo4j backend (opt-in feature)

**Where to verify**: `crates/epica-memory/src/neo4j/mod.rs`

```bash
cargo check -p epica-memory --features neo4j   # real neo4rs 0.8 impl compiles
```

`Neo4jMemoryStore::connect()` returns a live `Graph` against `neo4rs 0.8`.
Feature is opt-in — the default workspace build does not pull in `neo4rs`.

**Known limitation**: live smoke test against a real Neo4j server is not
in CI (TD-P8-004). The implementation compiles and the driver is wired;
exercising it requires a CI runner with a Neo4j sidecar.

---

### Claim: Reproducible benchmark harness with 4 headline metrics

**Where to verify**: `crates/epica-benchmarks/`, `docs/benchmarks/`

```bash
cargo build --release -p epica-benchmarks --bin epica-bench
target/release/epica-bench run-all --trajectories 200 --out-dir docs/benchmarks
# Diff the CSVs against docs/benchmarks/*.csv — identical byte-for-byte
```

Results at 200 trajectories per suite (FEP monitor enabled):

| Suite | T-ECE | AGM contradictions | FE mean (nats) | p99 lat (µs) |
|---|---:|---:|---:|---:|
| `alfworld_like` | 0.080 | 0 | 1.88 | 79 |
| `webshop_like` | 0.658 | 165 | 1.85 | 253 |

**Honest scope**: current numbers are from synthetic trajectory generators;
real ALFWorld / WebShop adapters are deferred (TD-P13-001). See
`docs/benchmarks/README.md` for the full honest-scope statement.

---

## What is conceptual vs. truth surface

### Conceptual documents (design rationale, paper mapping)

- `docs/architecture.md` — explains design choices; implementation pointers included
- `docs/dual_process.md` — explains paper-to-runtime mapping; divergences documented
- `docs/contracts.md` — explains `C = (P, I, G, R)` and enforcement flow

### Truth surface (operational state)

- `docs/evidence.md` — exact test counts, benchmark results, unverified claims
- `docs/phase_roadmap.md` — per-phase / per-sprint status with verification commands
- `docs/mcp_server.md` — per-endpoint status table
- `docs/agm_postulates.md` — per-postulate exact vs. approximate table

---

## What is experimental vs. stable

### Stable core (Phases 1–3, Sprints 1–4)

- `epica-core`: BeliefQuad, AGM K\*2–K\*6, System 1, `EmbeddingProvider`
  trait — fully tested, no known breaking changes planned.
- `epica-contracts`: `BehavioralContract`, `MnemonicSovereignty`,
  `AuditLedger` (BLAKE3 Merkle) — implemented and tested.
- `epica-macros`: `#[derive(BeliefState)]` — implemented with trybuild tests.
- `epica-zk-evidence`: `EvidenceReceipt`, Ed25519 prover/verifier,
  `epica-verify` CLI — stable wire format (schema-versioned JSON).

### Stable with known gaps (Phases 4–7)

- `epica-anthropic`: implemented; live calls depend on `ANTHROPIC_API_KEY`.
- `epica-openai`: `OpenAiLlmClient` + `OpenAiEmbeddingProvider`; live
  calls depend on `OPENAI_API_KEY`; embedding cache is unbounded (TD-P8-002).
- `epica-mcp`: full implementation; 29 E2E tests pass.
- `epica-python`: full API including `LlmClient` injection; no native async
  bridge (TD-P6-001).
- `epica-memory`: Redis fully works; Neo4j compiles and wires but is not
  exercised against a live server in CI (TD-P8-004).

### Opt-in feature paths (compile-verified, not in default build)

- `epica-active-inference` (feature `active-inference`): FEP monitor — 16
  unit + 5 integration tests; not in default workspace members.
- `epica-memory --features neo4j`: real neo4rs driver — compile-verified
  only in CI.
- `epica-zk-evidence --features risc0`: documented skeleton only
  (`zk_skeleton.rs`) — not functional.

---

## Anticipated objections

**"K\*6 is not satisfied"** — K\*6 is now a real semantic postulate when
an `EmbeddingProvider` is installed. When no provider is installed, the
fallback to structural equality is identical to the pre-Sprint-1 behaviour,
which satisfies K\*6 for any belief set with structurally-equal values.
The `Asserted/Asserted` scope limitation is documented as TD-P8-003.

**"PostulateAudit doesn't block mutations"** — This was true in Phase 1;
it changed at Sprint 1. K\*2, K\*3, K\*5, and K\*6 now reject the
revision with `BeliefRevisionError::PostulateViolation`. See
`agm.rs::revise()` and the rejection tests in `k2_success.rs` et al.

**"The benchmark is deterministic, not real-world"** — Correct and
documented. The `epica-bench` harness validates the T-ECE computation
pipeline, the FEP hook, and AGM contradiction detection on realistic
epistemic patterns. Real-world calibration against live ALFWorld / WebShop
is the next step (TD-P13-001, `RealEnvAdapter` trait is the seam).

**"ZK proofs are missing"** — The audit ledger delivers tamper-evidence,
non-repudiation, and offline verifiability today via Ed25519 over the
Merkle root. ZK adds *privacy* (proving AGM validity without revealing the
beliefs), not *correctness*. The RISC Zero skeleton (feature `risc0`) is
the documented upgrade path. See `DEVLOG.md § Sprint 3.2` for the
cost/value reasoning.

**"Neo4j is not exercised in CI"** — Correct. The `neo4rs 0.8` driver
compiles and connects; a live smoke test requires a CI runner with a Neo4j
sidecar (TD-P8-004).

**"System 2 tasks always return Completed"** — This was true earlier (TD-P5-002
was the open item). Tasks now use `SledTaskStore` for persistence and return
a real `Pending` state when System 2 is active. Verified in `e2e_tasks.rs`.

**"Active Inference doesn't map to real LLM internals"** — Correct by
design. The mapping is over the `BeliefQuad` (the agent's model of the
world), not LLM attention weights. The paper is substrate-agnostic; the
mapping is documented honestly in
`crates/epica-active-inference/src/lib.rs`. What is measured is
*whole-agent* calibration drift, not per-token uncertainty.
