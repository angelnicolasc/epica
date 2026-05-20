# Epica

[![CI](https://github.com/angelnicolasc/epica/actions/workflows/ci.yml/badge.svg)](https://github.com/angelnicolasc/epica/actions/workflows/ci.yml)
[![Security Audit](https://github.com/angelnicolasc/epica/actions/workflows/audit.yml/badge.svg)](https://github.com/angelnicolasc/epica/actions/workflows/audit.yml)
[![Supply Chain](https://github.com/angelnicolasc/epica/actions/workflows/deny.yml/badge.svg)](https://github.com/angelnicolasc/epica/actions/workflows/deny.yml)
[![Rust: stable](https://img.shields.io/badge/rust-stable-orange.svg)](https://www.rust-lang.org/)
[![MSRV: 1.82](https://img.shields.io/badge/MSRV-1.82-blue.svg)](https://blog.rust-lang.org/2024/10/17/Rust-1.82.0.html)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

**Epistemic Causal Agent Belief Runtime** — an embeddable Rust library for formal causal belief revision in LLM agents.

---

## What Epica does

Epica gives LLM agents a disciplined belief layer: a typed, revision-safe,
contract-governed store with a cryptographically auditable trail and a
continuous Bayesian-surprise monitor on top. Grounded in five 2026 arXiv
papers, implemented across **12 Rust crates + 2 CLI binaries**, fully
tested end-to-end.

Four concrete capabilities this codebase delivers today:

- **Contradiction-aware belief updates with semantic-equivalence support.**
  When new evidence contradicts an existing belief on the same key, AGM
  contraction (Alchourrón-Gärdenfors-Makinson 1985) applies a
  Core-Retainment-style minimal contraction over the belief's supporting
  premises before accepting the new value. Postulates **K\*2–K\*6** are
  enforced as hard errors in all builds — K\*6 (extensionality) is the
  real semantic postulate, not a structural stub: when an
  `EmbeddingProvider` is installed, paraphrases of the same intent are
  recognised as equivalent and do not trigger spurious revisions.

- **Dual-process uncertainty with continuous Bayesian-surprise monitor.**
  System 1 propagates confidence through the causal graph via Noisy-OR.
  System 2 triggers async LLM reflection when confidence diverges from
  the reliability baseline. On top of those, an optional
  [Active Inference / Free Energy](crates/epica-active-inference/)
  monitor (Friston) tracks per-observation surprise against a
  homeostatic budget — the runtime can detect *whole-agent* drift that
  no individual contract invariant catches.

- **Tamper-evident audit trail with third-party-verifiable receipts.**
  Every governance event is sealed into a BLAKE3 Merkle hash chain
  ([`AuditLedger`](crates/epica-contracts/src/sovereignty/ledger.rs)).
  The [`epica-verify`](crates/epica-zk-evidence/) CLI emits and
  validates Ed25519-signed [`EvidenceReceipt`](crates/epica-zk-evidence/src/receipt.rs)s
  — auditors can confirm offline that a ledger window was sealed by a
  specific prover and has not been edited since, with O(log N) per-entry
  Merkle proofs.

- **Reproducible benchmark harness with the 4 headline metrics.**
  The [`epica-bench`](crates/epica-benchmarks/) CLI runs deterministic
  ALFWorld-/WebShop-style synthetic trajectories and emits CSV +
  Markdown reports of **BeliefShift (T-ECE)**, **contract violations**,
  **free-energy mean**, and **insert latency p50/p95/p99**. Numbers in
  `docs/benchmarks/` are reproducible bit-for-bit across hosts.

---

## Why this matters

LLM agents accumulate beliefs across tool calls, user turns, and
environment observations. Standard memory layers (vector stores, session
state, KV caches) do not:

- detect when a new belief semantically contradicts existing ones,
- propagate confidence changes through causal dependencies,
- enforce typed policies on who may write, revise, or delete a belief,
- produce an auditable trail an external party can verify without trust.

Epica addresses these gaps with a runtime designed for deployment
alongside any LLM framework (Anthropic SDK, OpenAI SDK, LangGraph,
MCP-compatible hosts).

---

## Workspace map

12 crates + 2 CLI binaries, all part of the default `cargo check
--workspace --exclude epica-python` build.

| Crate | Role | Status |
|---|---|---|
| [`epica-core`](crates/epica-core/) | `BeliefQuad`, 4 orthogonal graphs, AGM K\*2–K\*6, System 1 Noisy-OR, checkpoint/rollback, `EmbeddingProvider` trait | ✅ |
| [`epica-runtime`](crates/epica-runtime/) | `BeliefRuntime`, dual-process System 1/2, `LlmClient` trait, `ConfidenceHistory`, T-ECE, retrieval, FEP hook | ✅ |
| [`epica-contracts`](crates/epica-contracts/) | `BehavioralContract` C=(P,I,G,R), 9 Mnemonic Sovereignty primitives, `AuditLedger` (Merkle + BLAKE3) | ✅ |
| [`epica-macros`](crates/epica-macros/) | `#[derive(BeliefState)]` proc-macro, 9 attributes | ✅ |
| [`epica-anthropic`](crates/epica-anthropic/) | `AnthropicLlmClient` + `ProspectiveClient` over Anthropic Messages API | ✅ |
| [`epica-openai`](crates/epica-openai/) | `OpenAiLlmClient` + `OpenAiEmbeddingProvider` over OpenAI-compatible APIs | ✅ |
| [`epica-active-inference`](crates/epica-active-inference/) | `ActiveInferenceMonitor` — variational free-energy over the BeliefQuad | ✅ (opt-in feature) |
| [`epica-mcp`](crates/epica-mcp/) | Axum MCP 2026 server, 16 routes, SEP-1686 Tasks, OAuth 2.1 JWT, Prometheus | ✅ |
| [`epica-memory`](crates/epica-memory/) | `LongTermMemoryStore` trait + Redis + Neo4j (`neo4rs 0.8`) backends | ✅ |
| [`epica-python`](crates/epica-python/) | PyO3 SDK: `PyBeliefQuad`, `PyBeliefRuntime`, contracts, `PyMockLlmClient`, `LlmClientHandle` | ✅ |
| [`epica-zk-evidence`](crates/epica-zk-evidence/) | `EvidenceReceipt` + Ed25519 prover/verifier + `epica-verify` CLI | ✅ |
| [`epica-benchmarks`](crates/epica-benchmarks/) | Synthetic ALFWorld/WebShop traces + `epica-bench` CLI | ✅ (workspace-internal) |

CLIs:

- [`epica-serve`](crates/epica-mcp/src/main.rs) — MCP 2026 server.
- [`epica-verify`](crates/epica-zk-evidence/src/bin/verify.rs) — `keygen` / `seal` / `verify` audit receipts.
- [`epica-bench`](crates/epica-benchmarks/src/bin/bench.rs) — benchmark suites with CSV / Markdown output.

---

## What every claim costs to verify

```bash
# Full workspace, default features
cargo test --workspace --exclude epica-python                  # ~500 tests across all crates

# Per-feature paths
cargo check  -p epica-memory          --features neo4j         # real neo4rs driver compiles
cargo check  -p epica-runtime         --features active-inference
cargo check  -p epica-zk-evidence     --features risc0         # documented skeleton
cargo test   -p epica-runtime         --features active-inference

# Python SDK (requires Python + maturin)
cd crates/epica-python && maturin develop
python -m pytest tests/ -v                                     # 65 pytest

# Reproduce published bench artefacts
cargo build --release -p epica-benchmarks --bin epica-bench
target/release/epica-bench run-all --trajectories 200 --out-dir docs/benchmarks
```

See [`docs/audit_guide.md`](docs/audit_guide.md) for the claim →
implementation → test map an external reviewer can walk top-to-bottom.

---

## Capability table

| Capability | Status | Verification path |
|---|---|---|
| BeliefQuad 4-graph store | ✅ | `cargo test -p epica-core` |
| AGM K\*2–K\*5 (proptest, 256+ cases each) | ✅ | `tests/agm_postulates/k{2,3,4,5}_*.rs` |
| **AGM K\*6 — semantic equivalence via `EmbeddingProvider`** | ✅ | `tests/agm_postulates/k6_extensionality.rs` (4 cases, including paraphrase + anti-parallel) |
| System 1 Noisy-OR with cycle guard | ✅ | `crates/epica-core/src/system1/`, proptest invariants |
| System 2 async reflection (token-bucket budget) | ✅ | `tests/system2_mock.rs`, `tests/system2_real_async.rs` |
| **Active Inference / Free Energy monitor** (opt-in) | ✅ | `cargo test -p epica-active-inference` (16) + `--features active-inference` integration (5) |
| T-ECE calibration metric + history | ✅ | `tests/beliefshift_benchmark.rs`, `tests/tece_session.rs` |
| Checkpoint / rollback with K\*4 guard | ✅ | `crates/epica-core/tests/integration/` |
| Behavioral contracts C=(P,I,G,R) | ✅ | `cargo test -p epica-contracts` |
| 9 Mnemonic Sovereignty primitives | ✅ | `crates/epica-contracts/tests/sovereignty.rs` |
| **Tamper-evident Merkle audit ledger (BLAKE3)** | ✅ | `cargo test -p epica-contracts` (`audit_ledger.rs` + ledger unit tests) |
| **Ed25519 EvidenceReceipt + `epica-verify` CLI** | ✅ | `cargo test -p epica-zk-evidence` (23: 18 unit + 4 CLI smoke + 1 doctest) |
| `#[derive(BeliefState)]` proc macro | ✅ | `cargo test -p epica-macros` |
| ProspectiveIndex with real LLM embedder | ✅ | `crates/epica-runtime/src/prospective/`, `epica-anthropic` impl |
| MCP 2026 server (16 routes) + SEP-1686 Tasks | ✅ | `cargo test -p epica-mcp` (29 E2E) |
| OAuth 2.1 JWT (HS256 / RS256) + JWKS rotation | ✅ | `tests/e2e_health.rs` |
| Prometheus metrics, OTLP optional | ✅ | `GET /metrics`, [`docs/observability.md`](docs/observability.md) |
| Python SDK + `LlmClient` injection from Python | ✅ | 65 pytest + `PyMockLlmClient.handle()` |
| **Redis + Neo4j persistence (both real, opt-in)** | ✅ | `cargo check -p epica-memory --features neo4j` |
| **`OpenAiEmbeddingProvider` (OpenAI-compatible)** | ✅ | `cargo test -p epica-openai` (17, with wiremock) |
| **Synthetic ALFWorld/WebShop benchmark harness** | ✅ | `target/release/epica-bench run-all` — see `docs/benchmarks/` |

The capabilities marked **bold** are post-public-review hardening
deliverables — see the [commit history](https://github.com/angelnicolasc/epica/commits/main)
for sprint-by-sprint disclosure of what was added, what was pivoted,
and why.

---

## Honest scope: what is *deliberately* deferred

Every shipping decision below was a documented pivot, not a missing
feature. Each one has a `RealEnvAdapter`-style seam ready for landing
when the infrastructure pre-requisites are in place.

| Item | What ships today | What needs infrastructure |
|---|---|---|
| **Real ALFWorld / WebShop benchmarks** | Synthetic traces emulating the *epistemic shape* — multi-step goals, paraphrases, filter-driven contradictions. Numbers reproducible bit-for-bit. | Python env + AI2-THOR install + Flask sim. Trait `RealEnvAdapter` is the seam ([code](crates/epica-benchmarks/src/real_adapters.rs)). |
| **ZK proofs over the audit ledger** | BLAKE3 Merkle commitment + Ed25519 signature → tamper-evidence + non-repudiation + offline verification. `EvidenceReceipt` wire format is stable. | RISC-V toolchain (`cargo-risczero` + `riscv32im-risc0-zkvm-elf`). Skeleton at [`zk_skeleton.rs`](crates/epica-zk-evidence/src/zk_skeleton.rs); feature `risc0`. |
| **Native Python `await` bridge** | Sync Python SDK fully wired, including `LlmClient` injection via `MockLlmClient`. | `pyo3-asyncio` stable for pyo3 0.22 (TD-P6-001). |

These pivots are documented in the seam module headers linked above
with the cost / value reasoning. None of them is hidden behind silent
stubs — each seam is a published trait + an `_AVAILABLE: bool` flag.

---

## Quick start

### Rust

```rust
use epica_core::{BeliefQuad, BeliefNode, BeliefValue, Provenance};

let mut quad = BeliefQuad::new();
let id = quad.insert(BeliefNode::new(
    "user_intent",
    BeliefValue::Asserted("refactor auth module".into()),
    Provenance::LlmInference { model: "claude-sonnet-4-6".into(), call_id: uuid::Uuid::new_v4(), prompt_hash: 0 },
    0.85,
));

let record = quad.revise(
    id,
    BeliefValue::Asserted("refactor the auth subsystem".into()),  // paraphrase
    Provenance::UserStatement { turn: 1 },
    0.90,
).expect("AGM revision");

// Postulate audit accompanies every revision.
assert!(record.postulate_audit.all_critical_pass());
assert!(record.postulate_audit.extensionality, "K*6 holds");
```

When an `EmbeddingProvider` is installed via
`quad.set_embedding_provider(...)`, the paraphrase above is recognised
as semantically equivalent — K\*6 fires `SemanticEquivalent` and no
contraction is triggered.

### Python

```python
from epica import BeliefRuntime, MockLlmClient

with BeliefRuntime(reflection_threshold=0.15, budget=50) as rt:
    rt.attach_llm_client(MockLlmClient(revised_confidence=0.7).handle())
    rt.insert_belief("user_goal", "deploy service", 0.9)
    rt.update_belief("user_goal", "deploy cancelled", 0.3)
    report = rt.finalize_session()
    print(f"T-ECE: {report.trajectory_ece}")
```

### Audit a sealed ledger

```bash
# Producer (CI / agent process)
epica-verify keygen --secret-out prover.hex
epica-verify seal --ledger ledger.json --secret prover.hex --out receipt.json

# Auditor (offline, no network needed)
epica-verify verify --ledger ledger.json --receipt receipt.json
# OK: receipt verifies — entries 0..=999 of 1000 sealed by abc123def...
```

---

## Theoretical foundation

Five 2026 arXiv papers ground the architecture:

| Paper | Insight implemented |
|---|---|
| [MAGMA (2601.03236)](https://arxiv.org/abs/2601.03236) | Four orthogonal graphs instead of one monolithic graph |
| [Kumiho (2603.17244)](https://arxiv.org/abs/2603.17244) | Property graph operations correspond to AGM postulates; prospective indexing |
| [Agentic UQ (2601.15703)](https://arxiv.org/abs/2601.15703) | Dual-process uncertainty as control (not sensor); Trajectory-ECE metric |
| [Agent Behavioral Contracts (2602.22302)](https://arxiv.org/abs/2602.22302) | Formal C=(P,I,G,R) contracts with (p,δ,k)-satisfaction bounds |
| [Mnemonic Sovereignty (2604.16548)](https://arxiv.org/abs/2604.16548) | Nine memory governance primitives as enforcement invariants |

Plus, post-public-review hardening drew on:

- [Friston, Free Energy Principle](https://www.fil.ion.ucl.ac.uk/~karl/) (lineage: [pymdp](https://github.com/infer-actively/pymdp), [RxInfer.jl](https://github.com/biaslab/RxInfer.jl)) — the `ActiveInferenceMonitor` ([code](crates/epica-active-inference/src/lib.rs)).
- [Alchourrón, Gärdenfors, Makinson 1985](https://en.wikipedia.org/wiki/AGM_postulates) + [Hansson Core-Retainment](https://link.springer.com/chapter/10.1007/978-94-017-1278-0_3) — the AGM revision foundation.

---

## Documentation

| Document | Covers |
|---|---|
| [`docs/architecture.md`](docs/architecture.md) | Four-graph design, invariants, data flow, design tradeoffs |
| [`docs/agm_postulates.md`](docs/agm_postulates.md) | Postulate-by-postulate compliance, including the real K\*6 path |
| [`docs/dual_process.md`](docs/dual_process.md) | System 1 / System 2 mechanics, T-ECE benchmark |
| [`docs/contracts.md`](docs/contracts.md) | C=(P,I,G,R) components, runtime enforcement points |
| [`docs/mnemonic_sovereignty.md`](docs/mnemonic_sovereignty.md) | Nine governance primitives, forget verification |
| [`docs/evidence.md`](docs/evidence.md) | Test inventory + benchmark results, what is unverified |
| [`docs/audit_guide.md`](docs/audit_guide.md) | Claim → implementation → test mapping (hostile-reviewer optimised) |
| [`docs/phase_roadmap.md`](docs/phase_roadmap.md) | Per-phase / per-sprint status + verification commands |
| [`docs/non_goals.md`](docs/non_goals.md) | What Epica deliberately does not attempt |
| [`docs/mcp_server.md`](docs/mcp_server.md) | Endpoint table with per-route implementation status |
| [`docs/end_to_end_example.md`](docs/end_to_end_example.md) | Narrative walkthrough |
| [`docs/observability.md`](docs/observability.md) | Prometheus + OTLP setup |
| [`docs/fuzzing.md`](docs/fuzzing.md) | libFuzzer targets |
| [`docs/competitive_landscape.md`](docs/competitive_landscape.md) | Honest comparison against alternatives |
| [`docs/benchmarks/README.md`](docs/benchmarks/README.md) | Sprint-4 harness, scope, reproduction |
| [`ROADMAP.md`](ROADMAP.md) | Canonical roadmap, post-Sprint-4 state |
| [`BENCHMARKS.md`](BENCHMARKS.md) | Performance numbers (Criterion + harness) |

---

## Visualising a `BeliefQuad`

`epica_core::quad::viz::to_dot(&quad)` returns a
[Graphviz DOT](https://graphviz.org/) document with nodes colour-coded
by `fast_confidence` and edges styled per relationship type.

```bash
cargo run --example visualize_quad > out.dot
dot -Tsvg out.dot > out.svg     # requires graphviz on PATH
```

The MCP server exposes the same serialisation live at
`GET /v1/visualize/dot` (Content-Type `text/vnd.graphviz`):

```bash
curl http://localhost:8765/v1/visualize/dot | dot -Tsvg > current.svg
```

---

## What Epica does that typical memory layers do not

| Feature | Vector store | Graph memory | Session KV | **Epica** |
|---|:-:|:-:|:-:|:-:|
| Contradiction detection | ✗ | ✗ | ✗ | AGM Core-Retainment contraction |
| Semantic-equivalence K\*6 | ✗ | ✗ | ✗ | Embedding-aware on the hot path |
| Causal confidence propagation | ✗ | ✗ | ✗ | Noisy-OR over `CausalGraph` |
| Continuous Bayesian-surprise audit | ✗ | ✗ | ✗ | Friston FEP monitor (opt-in) |
| Formal revision postulates | ✗ | ✗ | ✗ | K\*2–K\*6 verified |
| Typed contracts on writes | ✗ | ✗ | ✗ | C=(P,I,G,R) |
| Memory governance primitives | ✗ | ✗ | ✗ | 9 primitives (arXiv:2604.16548) |
| MCP 2026 native | varies | ✗ | ✗ | 16 routes + SEP-1686 Tasks |
| Tamper-evident audit ledger | ✗ | ✗ | ✗ | BLAKE3 Merkle chain + Ed25519 receipts |
| Offline third-party verifiability | ✗ | ✗ | ✗ | `epica-verify` CLI |
| Rollback with AGM guard | ✗ | ✗ | ✗ | K\*4 vacuity enforced |

---

## Contributing

`cargo check --workspace --exclude crates/epica-python` must pass.
`cargo clippy --workspace --exclude crates/epica-python` must pass with
zero warnings on the default feature set.

See [`docs/architecture.md`](docs/architecture.md) for the invariants
you must preserve when touching `epica-core`.

---

## License

MIT
