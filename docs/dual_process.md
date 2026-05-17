# Dual-Process Uncertainty

Reference: Agentic UQ (arXiv:2601.15703, Salesforce AI Research).

## What this document covers

How Epica models and propagates uncertainty through System 1 (fast, causal) and System 2 (slow, LLM-reflective), where the implementation diverges from the paper, and what is empirically measured today.

## What is implemented today

- System 1: Noisy-OR propagation through the CausalGraph, running synchronously after every `revise()`
- System 2: threshold-triggered LLM reflection via `LlmClient` trait, implemented in `epica-anthropic`
- Trajectory-ECE metric: computed from `ConfidenceHistory` in `BeliefRuntime`
- T-ECE benchmark result: **0.07 < 0.08 target** on a deterministic 25-step session

## What is still approximate or planned

- System 2 calls are synchronous in the current runtime ([TD-P5-002](phase_roadmap.md#open-technical-debts))
- System 2 LLM is not accessible from Python ([TD-P7-002](phase_roadmap.md#open-technical-debts))
- T-ECE is validated on a deterministic benchmark; performance on real partial-observability sessions is not yet measured

---

## What is borrowed from the paper

The paper's Agentic Uncertainty Management (AUM) framework provides:

- The dual-process framing: System 1 (fast, cheap, always-on) and System 2 (slow, LLM-reflective, rate-limited)
- The divergence condition for System 2 activation: `|fast_confidence - reliability_baseline| > tau`
- The `tau ~= 0.15` calibration from ALFWorld and WebShop benchmarks
- The Trajectory-ECE (T-ECE) metric definition: `sum_t |confidence_t - accuracy_t| / T`

## What is different in Epica

The paper's System 1 (UAM - Uncertainty Absorption Module) propagates uncertainty through **internal Transformer attention weights**. Epica does not have access to model internals.

Epica implements an **external-runtime approximation**: confidence propagates through the *external* CausalGraph via Noisy-OR, applied to the belief graph the agent maintains, not to the model's internal activations.

This approximation is:
- Auditable - the propagation path is a traversable graph, inspectable via `counterfactual_query()`
- Deterministic - given the same graph structure, the same result every time
- Controllable - confidence floors, decay coefficients, and cycle guards are configurable
- Not equivalent to the paper - confidence from model internals and confidence from agent-maintained causal structure are different quantities

## Why the approximation is acceptable

In agentic settings, the bottleneck for belief reliability is not model-internal uncertainty but the agent's handling of contradictions, dependencies, and accumulation across turns. Noisy-OR over the causal graph captures the signal that matters at this layer: how much should a downstream belief be trusted given upstream confidence changes?

---

## System 1: fast path

`BeliefQuad::propagate_system1(changed: BeliefId)` - runs synchronously after every `revise()`.

```text
for each InferredFrom edge terminating at changed:
    noisy_or = 1 - product(1 - premise.fast_confidence)
per-node combined = max(noisy_or over all InferredFrom edges)
decay = exp(-1/ttl_ms * elapsed_ms)
changed.fast_confidence = combined * decay
recurse into causal descendants (cycle guard via HashSet)
```

Cost: O(descendants) amortized. Cycle guard is a `HashSet<BeliefId>` - non-optional (prevents infinite recursion on cyclic causal graphs).

Implemented in: `crates/epica-core/src/system1/mod.rs`  
Tested in: `crates/epica-core/tests/integration/system1_propagation.rs`

---

## System 2: slow path

`BeliefRuntime::update_belief()` checks after System 1:

```text
divergence = |fast_confidence - reliability_baseline|
if divergence > node.reflection_threshold AND budget.try_consume(1):
    drop write lock
    diagnostic = compute_diagnostic(id)
    result = llm_client.reflect(diagnostic)   # LLM call
    node.slow_confidence = result.revised_confidence
```

Rate-limited by `TokenBucket` (default: 50 reflections/session, refill rate configurable).

Without a configured `LlmClient`, `update_belief()` returns `System1Only` - no budget consumed, no LLM call.

Implemented in: `crates/epica-runtime/src/runtime.rs`, `crates/epica-anthropic/src/client.rs`  
Tested in: `crates/epica-runtime/tests/system2_mock.rs`

**Current limitation**: System 2 executes synchronously in the current implementation. The task created in the MCP server appears immediately as `Completed`. Genuine async System 2 (where the caller polls for the result) requires persistent task storage (TD-P5-002).

---

## Trajectory-ECE

```text
T-ECE = sum_t |confidence_t - accuracy_t| / T
```

Where `accuracy_t = 1.0` if the belief at step `t` was later confirmed correct by a tool result (ground truth set by `ConfidenceHistory::mark_correct()`), and `accuracy_t = 0.0` if the belief was contradicted.

Implemented in: `crates/epica-runtime/src/history.rs`, `crates/epica-core/src/diff/tece.rs`

### Empirical status

| Benchmark | Target | Measured result | How measured | Notes |
|-----------|--------|-----------------|--------------|-------|
| BeliefShift | T-ECE < 0.08 | **0.07** | `crates/epica-runtime/tests/beliefshift_benchmark.rs` | Deterministic 25-step session |

BeliefShift design: 22 beliefs correct at confidence 0.93 (`|0.93 - 1.0| = 0.07` each) + 3 beliefs incorrect at confidence 0.07 (`|0.07 - 0.0| = 0.07` each). T-ECE = `(25 * 0.07) / 25 = 0.07`.

This is a deterministic benchmark that validates the T-ECE computation pipeline. It does not measure calibration on real partial-observability tasks (ALFWorld, WebShop). Performance on realistic workloads is not yet measured.

---

## tau = 0.15

The paper reports `tau ~= 0.15` as optimal on ALFWorld and WebShop. This value is used as the default `reflection_threshold` on every `BeliefNode`. Override per-belief with `.with_reflection_threshold(tau)`.

This value is borrowed directly from the paper's empirical finding. It has not been independently validated on Epica's runtime. If you run in a domain with different base rates of belief correctness, you should tune this value and measure T-ECE on your own sessions.

---

## Current status

- System 1: implemented and tested
- System 2: implemented (sync); real LLM calls via `epica-anthropic`
- T-ECE: implemented; BeliefShift result = 0.07 (verified)
- Verified by: `cargo test -p epica-runtime --features system2`

## Known limitations

- System 2 is synchronous ([TD-P5-002](phase_roadmap.md#open-technical-debts))
- T-ECE validated only on deterministic benchmark, not on real partial-observability workloads
- `tau = 0.15` not independently calibrated for Epica's runtime
- Python SDK does not expose System 2 LLM injection ([TD-P7-002](phase_roadmap.md#open-technical-debts))
