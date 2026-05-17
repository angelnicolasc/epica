# Architecture

## What this document covers

Why Epica uses four orthogonal graphs instead of one, the invariants that hold across all graph operations, the data flow through the runtime, design tradeoffs made explicitly, and failure modes this architecture is designed to prevent.

---

## Why four graphs?

MAGMA (arXiv:2601.03236) runs ablations on a monolithic belief graph vs. four orthogonal graphs and demonstrates that:

1. **Retrieval ambiguity**: a single graph cannot distinguish "A happened before B" (temporal) from "A causes B" (causal) - both are edges. Retrieval must guess which semantic to apply.
2. **Traversal interference**: causal DFS visits semantically unrelated nodes that happen to share temporal edges.
3. **Interpretability collapse**: the meaning of a path through a monolithic graph is undefined without edge-type inspection at every step.

Epica's `BeliefQuad` maintains four `StableDiGraph<BeliefId, EdgeType>` instances synchronized over a single `SlotMap<BeliefId, BeliefNode>`.

Each graph has exactly one semantics:
- `SemanticGraph` - subsumes / contradicts / synonymous
- `TemporalGraph` - before / after (with temporal decay)
- `CausalGraph` - causes / is caused by (with effect size and Noisy-OR propagation)
- `EntityGraph` - entity membership / role assignment

---

## Invariants

### 1. BeliefId is the single source of truth

The four graphs store `BeliefId` as node weights - NOT copies of `BeliefNode`. This means:
- Modifying a node's value only requires touching the `SlotMap`.
- Graph traversal returns `BeliefId`s, which are then used for O(1) `SlotMap` lookup.
- Removing a node from the `SlotMap` doesn't break graph indices - the `NodeIndex` in each graph still points to the `BeliefId`, but lookup into the (now-empty) `SlotMap` will return `None`.

### 2. O(1) graph node lookup

Each graph struct maintains a `HashMap<BeliefId, petgraph::stable_graph::NodeIndex>`. Adding nodes to the graph goes through this indirection:

```rust
fn add_node(&mut self, id: BeliefId) -> NodeIndex {
    let idx = self.graph.add_node(id);
    self.indices.insert(id, idx);
    idx
}
```

Failure to maintain this map is the most common petgraph bug. All graph methods use `self.indices.get(&id)` and return early (`None`/empty) if the id is not registered.

### 3. StableGraph for safe removal

`petgraph::stable_graph::StableDiGraph` does NOT invalidate `NodeIndex` values when a node is removed. This is critical because:
- Removing a belief during AGM contraction (`apply_contraction()`) must not break the `NodeIndex` values stored in `indices` for other beliefs.
- Regular `petgraph::Graph` DOES invalidate indices on removal - using it would require rebuilding the entire `indices` map on every contraction.

### 4. System 1 cycle guard

The CausalGraph is user-constructed and CAN contain cycles (the API cannot prevent them without a DAG type constraint). `propagate_system1()` uses a `HashSet<BeliefId>` visited set:

```rust
fn propagate_system1_inner(&mut self, id: BeliefId, visited: &mut HashSet<BeliefId>) {
    if !visited.insert(id) { return; } // already visited - cycle guard
    // ...
}
```

This guard is non-optional. A cycle without it would cause infinite recursion.

### 5. PostulateAudit is an audit trail, not a gate

`PostulateAudit::verify()` captures the pre-revision quad state and checks all six postulates. It runs **before** the mutation and attaches to `RevisionRecord`. Debug builds `assert!` on violations; release builds are silent. The revision proceeds regardless - the audit is a record, not a gate.

If hard enforcement is required, wire a `BehavioralContract` invariant that reads `RevisionRecord.audit`.

### 6. ProspectiveIndex exists from Phase 1

The `ProspectiveIndex` field is present in `BeliefQuad` from Phase 1 even though `index_belief()` was a no-op until Phase 4. This stabilizes:
- Serialization format (adding a field in Phase 4 would break checkpoint compatibility)
- The API surface (callers can reference `prospective_index` without conditional compilation)

---

## Data flow

```text
user/tool -> BeliefNode -> BeliefQuad.insert()
                           |
                           v
                 [4 graphs add_node()]
                           |
                           v
            BeliefQuad.revise() [AGM K*4 check]
                  /                     \
                 v                       v
         expand_only()         contract + expand
                  \                     /
                   v                   v
               fast_confidence updated
                           |
                           v
      BeliefQuad.propagate_system1() [Noisy-OR through CausalGraph]
                           |
                           v
              BeliefRuntime: System 2?
       |fast_conf - baseline| > tau AND budget > 0
                           |
                           v
             LlmClient.reflect() -> slow_confidence
                           |
                           v
      BehavioralContract.check_invariants()
                           |
                           v
      ConfidenceHistory.push(id, effective_conf)
```

---

## Design tradeoffs

### External causal graph vs. internal model uncertainty

Epica propagates confidence through an external, agent-maintained causal graph (Noisy-OR). The paper's UAM module propagates through Transformer attention weights. These are different quantities. Epica's approach is auditable and deterministic; it does not track model-internal uncertainty at all.

**Tradeoff accepted**: interpretability and determinism over fidelity to the paper's uncertainty model.

### SlotMap for storage, petgraph for structure

`SlotMap` provides O(1) insert/remove/lookup with stable keys. `petgraph::StableDiGraph` provides efficient graph traversal with stable `NodeIndex`. The indirection (`BeliefId` -> `NodeIndex` via `HashMap`) adds one hashmap lookup per graph operation - acceptable for the stability guarantees.

**Tradeoff accepted**: one extra lookup per graph operation in exchange for safe removal and stable indices.

### Synchronous System 2

System 2 executes synchronously in `BeliefRuntime::update_belief()`. This simplifies the locking model (no background threads modifying the quad) but means a single System 2 call can block the update path for the duration of an LLM round trip.

**Tradeoff accepted**: simpler locking model over update latency under System 2 activation.

### Structural K\*6

K\*6 (extensionality) is approximated via structural equality. Detecting semantic equivalence would require an embedding lookup on the `revise()` hot path, which is currently synchronous.

**Tradeoff accepted**: fast synchronous revision over full AGM compliance.

### Audit-trail, not gate for PostulateAudit

PostulateAudit is non-blocking in release builds. A blocking PostulateAudit would risk aborting valid belief updates when the audit logic has bugs.

**Tradeoff accepted**: resilience over hard AGM enforcement. Contract invariants provide an alternative hard-enforcement mechanism.

---

## Failure modes this architecture prevents

| Failure mode | Prevention mechanism |
|-------------|---------------------|
| Agent contradicts itself silently | AGM `check_contradiction()` before every `revise()`; K\*4 vacuity guard |
| Confidence propagates through cycles | `propagate_system1()` cycle guard via `HashSet<BeliefId>` |
| Graph node indices invalidated after removal | `StableDiGraph` guarantees stable `NodeIndex` after removal |
| Checkpoint format breaks when new fields are added | `#[serde(default)]` on all new fields; `prospective_index` present from Phase 1 |
| System 2 budget leak in test environments | `TokenBucket` only consumes budget when `LlmClient` is present |
| Contract violations propagate silently | `BehavioralContract.check_invariants()` called after every `update_belief()` |
| Belief erasure confirmed by presence in graph | `ForgetPolicy.verify_fn` traverses all four graphs after erasure |

---

## What this architecture does not solve

- **Semantic equivalence across beliefs**: two beliefs with different strings but identical meaning are treated as distinct. This affects K\*6 compliance and contradiction detection (TD-003).
- **Distributed belief consistency**: if multiple agents share a `BeliefQuad` through the MCP server, there is no distributed transaction guarantee. Concurrent writes are serialized by the `RwLock` within a single process, but cross-process consistency is not provided.
- **Model-internal uncertainty**: Epica does not access LLM attention weights or logits. Confidence is agent-maintained, not model-intrinsic.
- **Formal proof of AGM compliance**: the implementation is tested against postulate definitions, not formally proven in a proof assistant.
- **Long-term causal structure learning**: the causal graph is populated by the agent or user, not learned from data. Epica does not learn causal structure.

---

## Implementation pointers

| Component | Location |
|-----------|----------|
| `BeliefQuad` | `crates/epica-core/src/quad/mod.rs` |
| Four graphs | `crates/epica-core/src/quad/{semantic,temporal,causal,entity}.rs` |
| AGM revision | `crates/epica-core/src/revision/agm.rs` |
| `PostulateAudit` | `crates/epica-core/src/revision/postulates.rs` |
| System 1 propagation | `crates/epica-core/src/system1/mod.rs` |
| `BeliefRuntime` | `crates/epica-runtime/src/runtime.rs` |
| `BehavioralContract` | `crates/epica-contracts/src/contract.rs` |
| `MnemonicSovereignty` | `crates/epica-contracts/src/sovereignty.rs` |
