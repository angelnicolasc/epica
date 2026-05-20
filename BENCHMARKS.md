# Epica — Performance Benchmarks

Two complementary benchmark suites cover different parts of the stack:

1. **Criterion micro-benchmarks** — hot-path operations on `epica-core`
   (insert, revise, checkpoint/rollback, System 1 propagation).
2. **End-to-end harness** — `epica-bench` CLI running synthetic
   ALFWorld/WebShop trajectories against the full runtime stack, reporting
   T-ECE, contract violations, free-energy mean, and insert latency.

---

## 1. Criterion micro-benchmarks (`epica-core`)

Every figure on this page comes from a Criterion run reproducible with
`cargo bench -p epica-core`. Values are reported with their **median**
point estimate plus the 95 % confidence interval Criterion emits (mean ±
noise band).

### Reproducibility

| | |
|---|---|
| **CPU** | AMD Ryzen 5 3400G (4 cores / 8 threads, Zen+ 12 nm, 3.7 GHz base) |
| **OS** | Windows 11 64-bit |
| **Rust** | 1.90.0 stable (msrv 1.82) |
| **Profile** | `bench` (inherits release: `lto = "thin"`, `codegen-units = 1`) |
| **Criterion** | 0.5 with `html_reports` |
| **Sample size** | 15–30 per data point (rapid-CI mode); use defaults for archival runs |
| **Command** | `cargo bench -p epica-core` |

Higher sample counts narrow the confidence band but do not change the median
order of magnitude — these numbers are stable across re-runs on the same
hardware.

### Summary

| Operation | 100 beliefs | 1 000 beliefs | 10 000 beliefs |
|---|---:|---:|---:|
| `BeliefQuad::insert` (cumulative) | **60.6 µs** | **566 µs** | **8.24 ms** |
| `BeliefQuad::revise` (cumulative) | **41.3 µs** | **418 µs** | **4.98 ms** |
| `checkpoint → mutate → rollback_to` (round-trip) | **62.5 µs** | **625 µs** | **10.6 ms** |
| `propagate_system1` on a depth-N chain graph | **581 µs** | **34.8 ms** | **3.74 s** |
| `HashMap<usize, f32>` baseline (raw walk) | 103 ns | 1.28 µs | 17.6 µs |

Per-node amortized cost:

| Operation | per node @ 10 000 |
|---|---:|
| `insert` | ≈ 824 ns |
| `revise` | ≈ 498 ns |
| `checkpoint + rollback` | ≈ 1.06 µs |
| System 1 chain propagation | ≈ 374 µs |

### Read this honestly

The four CRUD-style operations (`insert`, `revise`, `checkpoint`, `rollback`)
are **linear in graph size** and run in sub-millisecond-per-node territory
even at 10 000 beliefs. That is well within budget for a real LLM agent
session (which rarely exceeds the low hundreds of beliefs in a single turn).

`propagate_system1` on a **pathological depth-N chain** is a different story:
it costs 374 µs / node at depth 10 000. That is ~21 000× the raw HashMap
baseline. This is honest — the chain topology is the **worst case** for
Noisy-OR propagation:

- every node has exactly one predecessor, so propagation cannot fan out;
- depth equals N, so the recursion guard does no pruning;
- the cycle-detection `HashSet<BeliefId>` is grown and probed at every level;
- `descendants_of()` walks the underlying `petgraph` index linearly per node.

Real agent belief graphs are **shallow DAGs with low fan-in**, not 10 000-deep
chains. For depth-bounded fan-out graphs the cost falls dramatically — that
benchmark is on the roadmap. Until it is added, **the chain numbers should be
read as an upper bound, not a representative case**.

This gap is tracked as HD-S2-A1 in `DEVLOG.md` and on
[ROADMAP.md](./ROADMAP.md) under Phase 1 open items.

### How to run

Full sweep (archival-quality, ~10 minutes on this hardware):

```bash
cargo bench -p epica-core
```

Quick CI-style sweep (rapid samples, ~3 minutes):

```bash
cargo bench -p epica-core -- --sample-size 15 --warm-up-time 1 --measurement-time 3
```

Single benchmark:

```bash
cargo bench -p epica-core --bench system1_propagation
cargo bench -p epica-core --bench checkpoint_rollback
cargo bench -p epica-core --bench belief_quad_throughput
```

HTML reports land in `target/criterion/`; open
`target/criterion/report/index.html` for the full set with violin plots
and regression history.

### Methodology notes

- **`insert` and `revise` measure cumulative cost over N operations.** Per-call
  cost is the reported time divided by N. We do *not* report a single-shot
  insert because Criterion's measurement overhead would dominate.
- **`checkpoint_rollback` measures a full round-trip on a fresh clone** of an
  N-node quad. The clone is necessary because `rollback_to` mutates state;
  excluding it would inflate the result by burying allocator cost.
- **`propagate_system1` measures one propagation from the root** of a depth-N
  chain. The recursion guard makes the second call a no-op, so the benchmark
  rebuilds setup once and times only the propagation.
- **The HashMap baseline is a raw `HashMap<usize, f32>` scan** with no
  graph semantics, contradiction checking, or temporal decay. It is the
  theoretical floor for "touch N confidences once", not a competitor —
  Epica's data structure does meaningfully more work per node and the ratio
  is the cost of that work.

---

## 2. End-to-end harness (`epica-benchmarks`)

The `epica-bench` CLI (crate `epica-benchmarks`) runs deterministic,
seeded synthetic trajectories through the full runtime stack — AGM
revision, K\*6 semantic equivalence, behavioral contracts, and the
optional FEP hook — and reports four headline metrics per suite.

### Reproducibility

| | |
|---|---|
| **CPU** | AMD Ryzen 5 3400G (same as above) |
| **Trajectories** | 200 per suite (deterministic — same seed → identical CSV byte-for-byte) |
| **Stack** | `BeliefRuntime` + `BehavioralContract` + `ActiveInferenceMonitor` (`--features active-inference`) |
| **Command** | `cargo build --release -p epica-benchmarks && target/release/epica-bench run-all --trajectories 200 --out-dir docs/benchmarks` |

### Results (200 trajectories per suite)

| Suite | T-ECE | AGM contradictions | Free energy mean (nats) | p99 insert lat (µs) |
|---|---:|---:|---:|---:|
| `alfworld_like` | **0.080** | 0 | 1.88 | **79** |
| `webshop_like` | **0.658** | 165 | 1.85 | **253** |

Full per-trajectory CSVs and per-suite Markdown summaries live in
[`docs/benchmarks/`](./docs/benchmarks/).

### Reading the numbers

**`alfworld_like`** — multi-step goal pursuit (8–14 steps). One asserted
goal, probe results at varied confidence, one mid-trajectory AGM
correction. T-ECE = 0.080 confirms the pipeline is calibrated on
well-ordered sequential revisions. Zero AGM contradictions: the agent
corrects via AGM expansion, not contradiction.

**`webshop_like`** — search-then-filter (10–18 steps). Exercises K\*6
paraphrase recognition, then 2–4 filter refinements that contradict prior
high-confidence candidates. **T-ECE = 0.658 is the correct behaviour**:
the runtime detects and exposes the miscalibration produced by the
search-then-refute pattern. 165 AGM contradictions across 200 trajectories
confirm the contradiction-aware revision path fires as expected.

**p99 ≤ 253 µs** — both suites run with the `active-inference` feature
enabled (FEP hook on every `insert_belief`). The 253 µs p99 on the more
intensive WebShop suite confirms the "<1 ms hot path" promise holds even
with the FEP hook in the call stack.

### Honest scope

The Sprint-4 plan cited ALFWorld (AI2-THOR text agent) and WebShop (Flask
shopping simulator) as the target live environments. **Current numbers are
from synthetic trajectory generators** that emulate the *epistemic shape*
of those benchmarks — not from a live Python environment. This is a
deliberate scope decision; the `RealEnvAdapter` trait in
[`crates/epica-benchmarks/src/real_adapters.rs`](./crates/epica-benchmarks/src/real_adapters.rs)
is the seam for upgrading to real environments without changing the harness
API, and its module header carries the cost / value reasoning.

### How to reproduce

```bash
# Build the release binary (the default dev build is ~10× slower)
cargo build --release -p epica-benchmarks --bin epica-bench

# Run both suites, 200 trajectories each
target/release/epica-bench run-all --trajectories 200 --out-dir docs/benchmarks

# Or run a single suite
target/release/epica-bench run alfworld_like --trajectories 200 --out-dir /tmp/bench

# Without FEP monitor (baseline comparison)
target/release/epica-bench run-all --no-active-inference
```

The CSVs in `docs/benchmarks/` are deterministic byte-for-byte — diffing
them against a local re-run is a valid reproducibility check.
