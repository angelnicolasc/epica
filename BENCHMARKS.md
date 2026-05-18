# Epica — Performance Benchmarks

Measured numbers for the `epica-core` hot paths. Every figure on this page
comes from a Criterion run reproducible with `cargo bench -p epica-core`.
Values are reported with their **median** point estimate plus the 95 %
confidence interval Criterion emits (mean ± noise band).

## Reproducibility

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

---

## Summary

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

---

## Read this honestly

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

This gap is tracked as a hardening item in `DEVLOG.md` (HD-S2-A1) and on
[ROADMAP.md](./ROADMAP.md) under Phase 5 (performance hardening).

---

## How to run

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

---

## Methodology notes

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
