# Epica Python SDK

Formal AGM belief revision for LLM agents - Rust-powered, Python-native.

```bash
pip install epica
```

---

## Overview

Most agent memory systems are retrieval pipelines: store, embed, retrieve. Epica is different — it is a belief revision runtime. When new information contradicts what the agent already believes, Epica applies AGM postulates to resolve the contradiction, propagates the confidence change causally, and enforces typed behavioral contracts before the agent can act on inconsistent state.

`epica` exposes this runtime to Python via a PyO3 extension module. The Rust core gives you:

- **BeliefQuad** - four-graph belief store (semantic, temporal, causal, entity) with AGM K*2-K*6 compliance
- **BeliefRuntime** - dual-process System 1 (synchronous Noisy-OR) + System 2 (async non-blocking LLM reflection; injectable from Python via `PyLlmClientHandle`) with Trajectory-ECE session reporting
- **BehavioralContract** - typed `C=(P,I,G,R)` contract enforcement (arXiv:2602.22302)
- **Thread-safe** - internally `Arc<RwLock<>>` / `Arc<Mutex<>>` - safe for GIL-free Python 3.13+
- **Typed** - PEP 561 package with hand-crafted `.pyi` stubs for mypy/pyright

---

## Quick start

```python
from epica import BeliefQuad, BeliefRuntime, BehavioralContract

# -- Low-level: four-graph belief store ------------------------------------
quad = BeliefQuad()
quad.insert("user_intent", "deploy to staging", 0.95)
quad.insert("environment", "production", 0.80)
quad.add_causal_edge("environment", "user_intent")

cp = quad.checkpoint()
quad.revise("user_intent", "deploy to production", 0.70)

diff = quad.rollback_to(cp)
print(diff)  # BeliefDiff(+0 -0 ~1, contradictions=True)

# -- High-level: dual-process runtime --------------------------------------
with BeliefRuntime(reflection_threshold=0.15, budget=50) as rt:
    rt.insert_belief("user_goal", "ship feature X", 0.9)
    rt.update_belief("user_goal", "ship feature Y", 0.6)
    report = rt.finalize_session()
    print(f"T-ECE: {report.trajectory_ece}")
    print(f"Calibrated: {report.calibration_target_met}")

# -- Contracts: C=(P,I,G,R) -----------------------------------------------
contract = BehavioralContract("deployment_safety")
contract.add_precondition("environment", min_confidence=0.8)
contract.add_invariant("user_goal", min_confidence=0.5, severity="hard")
contract.check_preconditions_raising(quad)
```

---

## API Reference

### `BeliefQuad`

Four-graph belief store. All methods are thread-safe.

| Method | Signature | Returns | Raises |
|--------|-----------|---------|--------|
| `insert` | `(key, value, confidence, provenance=None)` | `None` | - |
| `get` | `(key)` | `BeliefNode \| None` | - |
| `remove` | `(key)` | `bool` | - |
| `revise` | `(key, new_value, confidence, provenance=None)` | `None` | `BeliefRevisionError` |
| `checkpoint` | `()` | `str` | - |
| `rollback_to` | `(checkpoint_id)` | `BeliefDiff` | `CheckpointError` |
| `list_checkpoints` | `()` | `list[str]` | - |
| `diff_with_checkpoint` | `(checkpoint_id)` | `BeliefDiff` | `CheckpointError` |
| `counterfactual` | `(key)` | `CounterfactualResult` | `BeliefRevisionError` |
| `add_semantic_edge` | `(from_key, to_key, edge_type)` | `None` | `ValueError`, `KeyError` |
| `add_causal_edge` | `(cause_key, effect_key, effect_size=1.0, confidence=1.0)` | `None` | `KeyError` |
| `add_temporal_edge` | `(earlier_key, later_key)` | `None` | `KeyError` |
| `keys` | `()` | `list[str]` | - |
| `values` | `()` | `list[BeliefNode]` | - |
| `items` | `()` | `list[tuple[str, BeliefNode]]` | - |

**Dunder methods**: `__len__`, `__contains__`, `__bool__`, `__repr__`

**`edge_type` values**: `"subsumes"`, `"contradicts"`, `"synonymous"`

**`provenance` values**: `"user"` (default), `"llm"`, `"tool"`

---

### `BeliefRuntime`

Dual-process runtime. System 2 is async and non-blocking; inject a `LlmClient` from Python via `attach_llm_client(MockLlmClient(...).handle())` or `attach_llm_client(handle)`. No Python-level `await` bridge yet ([TD-P6-001](phase_roadmap.md#open-technical-debts)) — poll via `GET /v1/tasks/:id` for results. Implements the context manager protocol.

| Method | Signature | Returns | Raises |
|--------|-----------|---------|--------|
| `__init__` | `(reflection_threshold=0.15, budget=50, refill_rate=1.0)` | - | `EpicaError` |
| `insert_belief` | `(key, value, confidence, provenance=None)` | `str` (key) | - |
| `get_by_key` | `(key)` | `str \| None` | - |
| `get_belief` | `(key)` | `BeliefNode \| None` | - |
| `update_belief` | `(key, new_value, confidence, provenance=None)` | `dict` | `KeyError`, `ContractViolationError` |
| `retrieve_for_query` | `(query, budget=1000)` | `list[tuple[str, float]]` | - |
| `checkpoint` | `()` | `str` | - |
| `rollback_to` | `(checkpoint_id)` | `BeliefDiff` | `CheckpointError` |
| `finalize_session` | `()` | `SessionReport` | - |
| `session_report` | `()` | `SessionReport` | - |

**`update_belief` return dict**:
```python
{"status": "system1_only" | "system2_activated" | "system2_throttled", "task_id": str | None}
```

**`retrieve_for_query` score**: `prospective_sim*0.45 + uncertainty_bonus*0.25 + causal_centrality*0.20 - decay*0.10`

---

### `BehavioralContract`

Typed `C=(P,I,G,R)` contract (arXiv:2602.22302).

| Method | Signature | Returns | Raises |
|--------|-----------|---------|--------|
| `__init__` | `(domain, alpha=0.05, gamma=0.5)` | - | - |
| `add_precondition` | `(key, min_confidence=0.5)` | `None` | - |
| `add_presence_precondition` | `(key)` | `None` | - |
| `add_invariant` | `(key, min_confidence=0.5, severity="hard")` | `None` | `ValueError` |
| `set_max_tokens` | `(limit)` | `None` | - |
| `check_preconditions` | `(quad)` | `bool` | - |
| `check_preconditions_raising` | `(quad)` | `None` | `ContractViolationError` |
| `check_invariants` | `(quad)` | `bool` | - |
| `check_invariants_raising` | `(quad)` | `None` | `ContractViolationError` |
| `drift_bound` | `()` | `float` (D* = alpha/gamma) | - |

**Properties**: `domain`, `precondition_count`, `invariant_count`

**`severity` values**: `"soft"`, `"hard"` (default), `"critical"`

---

### Data classes (read-only)

#### `BeliefNode`
| Field | Type | Description |
|-------|------|-------------|
| `key` | `str` | Domain key |
| `value` | `str` | Human-readable value |
| `value_kind` | `str` | `"asserted"` \| `"inferred"` \| `"deterministic"` \| `"reference"` |
| `provenance` | `str` | Origin of the belief |
| `fast_confidence` | `float` | System 1 confidence `[0, 1]` |
| `slow_confidence` | `float \| None` | System 2 confidence |
| `effective_confidence` | `float` | `slow_confidence` if set, else `fast_confidence` |
| `created_at_ms` | `int` | Unix epoch milliseconds |

#### `BeliefDiff`
| Field | Type | Description |
|-------|------|-------------|
| `added` | `list[str]` | Keys added since checkpoint |
| `removed` | `list[str]` | Keys removed since checkpoint |
| `modified` | `list[str]` | Keys with changed value or confidence |
| `has_contradictions` | `bool` | Any of added/removed/modified is non-empty |
| `trajectory_ece` | `float \| None` | T-ECE for this interval |

Methods: `is_empty()`, `to_dict()`, `__len__`

#### `SessionReport`
| Field | Type | Description |
|-------|------|-------------|
| `trajectory_ece` | `float \| None` | `sum(|conf_t - acc_t|) / T` |
| `total_revisions` | `int` | Total `update_belief()` calls |
| `contradictions_detected` | `int` | Revisions where AGM contraction fired |
| `system2_activations` | `int` | System 2 LLM calls |
| `system2_throttled` | `int` | System 2 budget misses |
| `calibration_target_met` | `bool` | `trajectory_ece < 0.08` |
| `soft_violations` | `int` | Soft invariant violations |
| `hard_violations` | `int` | Hard invariant violations |
| `critical_violations` | `int` | Critical invariant violations |

Methods: `to_dict()`

#### `CounterfactualResult`
| Field | Type | Description |
|-------|------|-------------|
| `removed_key` | `str` | Removed antecedent key |
| `surviving` | `list[str]` | Keys surviving in counterfactual world |
| `excluded_count` | `int` | Antecedent + causal descendants |

---

## Exception Hierarchy

```
Exception
+-- EpicaError
    +-- BeliefRevisionError    # AGM failure, key not found
    +-- ContractViolationError  # precondition / invariant failure
    +-- CheckpointError         # checkpoint not found, K*4 guard
    +-- SovereigntyError        # mnemonic governance violation
```

```python
from epica import EpicaError, BeliefRevisionError

try:
    quad.revise("nonexistent", "value", 0.9)
except BeliefRevisionError as e:
    print(f"Revision failed: {e}")
except EpicaError as e:
    print(f"Epica error: {e}")
```

---

## Decorators

### `@belief_state(contract=None)`

Attaches a `BeliefQuad` to every instance of the decorated class.

```python
from epica import BehavioralContract
from epica.decorators import belief_state, governed_by

safety = BehavioralContract("safety")
safety.add_precondition("approved", min_confidence=0.9)

@belief_state(contract=safety)
class DeploymentAgent:
    def __init__(self):
        self.belief_quad.insert("approved", "yes", 0.95)

    @governed_by(safety)
    def deploy(self) -> str:
        return "deployed"
```

After decoration, every instance has:
- `self.belief_quad` - a fresh `BeliefQuad`
- `self._belief_contract` - the attached contract
- `self.check_invariants()` - evaluates contract invariants

### `@governed_by(contract)`

Enforces a contract on a method:
1. Checks `contract.check_preconditions_raising(self.belief_quad)` before the call
2. Checks `contract.check_invariants_raising(self.belief_quad)` after the call

Raises `ContractViolationError` on any failure.

---

## Framework Integrations

### Anthropic SDK

```python
import anthropic
from epica.integrations.anthropic import AnthropicBeliefSession

client = anthropic.Anthropic()

with AnthropicBeliefSession(client, model="claude-sonnet-4-6") as session:
    reply = session.message("The capital of France is Paris.")
    reply2 = session.message("What year did World War II end?")
    report = session.runtime.session_report()
    print(f"Beliefs extracted: {len(session.runtime.retrieve_for_query('', budget=50_000))}")
```

Install: `pip install "epica[anthropic]"`

### LangChain / LangGraph

```python
from epica import BeliefRuntime
from epica.integrations.langchain import EpicaBeliefTool

runtime = BeliefRuntime()
tool = EpicaBeliefTool(runtime=runtime)

# Use as a LangChain tool
lc_tool = tool.as_langchain_tool()
# Add to agent: create_tool_calling_agent(llm, tools=[lc_tool], ...)

# Or call directly:
result = tool({"key": "user_goal", "value": "ship feature X", "confidence": 0.9})
print(result)  # "Belief recorded: 'user_goal' = 'ship feature X' (confidence=0.90)"
```

Install: `pip install "epica[langchain]"`

---

## Type Safety

The package ships a PEP 561 `py.typed` marker and hand-crafted `.pyi` stubs:

```bash
# mypy
mypy your_script.py

# pyright
pyright your_script.py
```

All public classes, methods, and return types are fully annotated.

---

## Building from Source

Requires [maturin](https://github.com/PyO3/maturin) and Rust 1.82+:

```bash
cd crates/epica-python
pip install maturin
maturin develop          # installs in development mode
maturin build --release  # builds a wheel
```

---

## Thread Safety

All classes are thread-safe and suitable for GIL-free Python 3.13+ usage:

- `BeliefQuad` wraps `std::sync::RwLock<QuadState>` - concurrent reads, exclusive writes
- `BeliefRuntime` wraps `Arc<std::sync::Mutex<(BeliefRuntime, tokio::runtime::Runtime)>>` - serialised access

```python
import threading
from epica import BeliefQuad

quad = BeliefQuad()

def insert_worker(i):
    quad.insert(f"key_{i}", f"value_{i}", 0.5)

threads = [threading.Thread(target=insert_worker, args=(i,)) for i in range(10)]
for t in threads:
    t.start()
for t in threads:
    t.join()

assert len(quad) == 10  # all inserts are atomic
```

---

## Known Technical Debts

| ID | Status | Description |
|----|--------|-------------|
| TD-P6-001 | Open | Async Python bridge via `pyo3-asyncio` - true non-blocking `await` from Python async functions |
| TD-P6-002 | **Resolved** | `BeliefQuad.__getitem__` / `__setitem__` / `__delitem__` / `__iter__` - full dict-like protocol implemented |
| TD-P6-003 | Open | Maturin auto-generated stubs to replace hand-crafted `.pyi` as the API stabilises |
| TD-P6-004 | **Resolved** | `provenance="llm"` now accepts `llm_model: str` parameter; `provenance="tool"` accepts `tool_name: str`. Format: `"llm_inference:{model}"`, `"tool_result:{tool}"` |
