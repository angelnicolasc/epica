# Epica Benchmarks

This directory holds the benchmark artefacts produced by the
`epica-bench` CLI (crate: `epica-benchmarks`).

## How to reproduce

```bash
cargo build --release -p epica-benchmarks --bin epica-bench
target/release/epica-bench run-all --trajectories 200 --out-dir docs/benchmarks
```

The CSVs and per-suite Markdown summaries are regenerated in place. The
generator is deterministic — for the same `(suite, trajectory_id)` you
get the same trace bit-for-bit on any host. The numbers in this folder
were produced on the development workstation noted in `DEVLOG.md`; CI
will reproduce them within microsecond noise on the latency metrics.

## What's measured

Each suite reports four headline metrics + bookkeeping:

| Metric | Source | Sprint that introduced it |
|---|---|---|
| **BeliefShift (T-ECE)** | `BeliefRuntime::compute_tece()` after `finalize_session()` | Phase 2 — the original calibration target |
| **Contract violations** | `SessionReport::{soft,hard,critical}_violations` | Phase 3 — `BehavioralContract` invariants |
| **Free-energy mean (nats)** | `ActiveInferenceMonitor::mean_free_energy()` at end of run | Sprint 2 — `epica-active-inference` |
| **Insert latency (p50 / p95 / p99 / max, µs)** | Wall-clock around each `insert_belief` / `update_belief` | Sprint 4 — benchmark harness |

Plus: total ops, total AGM contradictions detected, calibration target
hit-rate, wall-clock.

## What's *not* a real-environment number

The Sprint-4 plan mentions ALFWorld (AI2-THOR text agent) and WebShop
(Flask shopping simulator) as the live environments. **The current
report uses synthetic trajectory generators that emulate the
*epistemic shape* of those benchmarks** — multi-step goals,
contradiction sequences, paraphrase of intent across turns. The
generator is in `crates/epica-benchmarks/src/traces.rs`.

This is a deliberate scope decision documented in
[`crates/epica-benchmarks/src/real_adapters.rs`](../../crates/epica-benchmarks/src/real_adapters.rs),
whose module header carries the cost / value reasoning.
The honest summary:

- **Synthetic numbers are reproducible**: no Python env, no AI2-THOR
  install, no LLM cost, no flaky network. Anyone can `cargo run` them.
- **Synthetic numbers exercise the same runtime paths**: AGM revisions,
  K\*6 semantic equivalence in the WebShop suite, contract evaluation,
  free-energy observation hook. The numbers reflect *Epica's runtime
  behavior on representative patterns*, not "what an LLM did against
  Epica today."
- **Real ALFWorld / WebShop adapters will land** once the workspace
  acquires a CI runner with the Python toolchain provisioned. The
  `RealEnvAdapter` trait in `real_adapters.rs` is the seam, and the
  harness API stays unchanged.

## Reading the suites

### `alfworld_like` — multi-step goal pursuit

8–14 steps per trajectory. One Asserted goal, 5–9 probe results
(Inferred at varied confidence), one mid-trajectory AGM revision when
the agent corrects an earlier wrong probe, and a final resolution.

Expectation: low T-ECE (~0.07–0.08), bounded AGM activity (the goal
itself doesn't contradict), modest free energy (the agent's posterior
mostly agrees with the structural prior).

### `webshop_like` — search-then-filter-then-purchase

10–18 steps per trajectory. Asserted user intent, 4–8 candidate
products, a paraphrase of the intent (exercises K\*6), 2–4 filter
refinements that contradict prior candidates, and a final purchase.

Expectation: higher T-ECE than `alfworld_like`, *because* the filter
mechanic deliberately produces miscalibrated early candidates whose
confidence is later refuted — that's the canonical WebShop failure
mode. A well-functioning runtime *should* surface this as T-ECE > 0.5,
and AGM contradictions in the hundreds across 200 trajectories.

Reading **high** T-ECE on `webshop_like` as a *good sign*: it means
the runtime's calibration metric is detecting the search-then-refute
pattern, not papering over it.

## File layout

```
docs/benchmarks/
├── README.md                            ← this file
├── alfworld_like.md                     ← Markdown summary (human-readable)
├── alfworld_like_summary.csv            ← one-row aggregate (machine-readable)
├── alfworld_like_per_trajectory.csv     ← N rows, one per trajectory
├── webshop_like.md
├── webshop_like_summary.csv
└── webshop_like_per_trajectory.csv
```

Both CSVs follow RFC 4180 with a header row.
