# End-to-End Example

This walkthrough traces a single belief through the full Epica runtime: from insertion, through contradiction and AGM contraction, confidence propagation, contract enforcement, and rollback.

---

## Scenario

A deployment agent is planning a release. It holds two beliefs:
- `environment` = "staging" (confidence 0.90, asserted by user)
- `deploy_target` = "staging" (confidence 0.85, inferred from `environment`)

A tool result arrives saying the environment has been promoted to production. This contradicts the existing `deploy_target` belief. Epica must:

1. Detect the contradiction
2. Contract minimum beliefs via AGM
3. Propagate the confidence change to `deploy_target`
4. Enforce the deployment safety contract (min confidence 0.8 on `environment`)
5. Checkpoint and rollback if needed

---

## Step 1: Initial beliefs

```rust
use epica_core::{BeliefQuad, BeliefNode, BeliefValue, Provenance};

let mut quad = BeliefQuad::new();

// User asserts environment = staging
let env_id = quad.insert(BeliefNode::new(
    "environment",
    BeliefValue::Asserted(serde_json::json!("staging")),
    Provenance::UserStatement,
    0.90,
));

// Agent infers deploy_target from environment
let target_id = quad.insert(BeliefNode::new(
    "deploy_target",
    BeliefValue::Inferred(serde_json::json!("staging")),
    Provenance::LlmInference {
        model: "claude-sonnet-4-6".into(),
        call_id: uuid::Uuid::new_v4(),
        prompt_hash: 0,
    },
    0.85,
));

// Wire the causal dependency: environment -> deploy_target
quad.add_causal_edge(env_id, target_id, 0.95, 0.90).unwrap();
```

State after Step 1:
- `environment`: 0.90 confidence, `Asserted`
- `deploy_target`: 0.85 confidence, `Inferred`, causally dependent on `environment`

---

## Step 2: Checkpoint before the update

```rust
let checkpoint_id = quad.checkpoint();
// checkpoint_id: "chk:550e8400-e29b-41d4-a716-446655440000"
```

The checkpoint is an immutable snapshot of the current quad state. If the revision turns out to be wrong, we can roll back with K\*4 guard.

---

## Step 3: Tool result arrives - environment = production

```rust
let revision = quad.revise(
    env_id,
    BeliefValue::Asserted(serde_json::json!("production")),
    Provenance::ToolResult { tool_name: "env_check".into(), call_id: uuid::Uuid::new_v4() },
    0.95,
).unwrap();
```

Inside `revise()`:

**3a. Contradiction detection**

`check_contradiction(env_id, "production")` compares the new value against existing beliefs. `deploy_target` is inferred from `environment` via `InferredFrom` edge. The new `environment = "production"` contradicts `deploy_target = "staging"` (same key cluster, different values).

Contradiction detected -> K\*4 vacuity guard does NOT apply -> contraction proceeds.

**3b. AGM contraction**

`minimal_contraction_set(env_id)` traverses `InferredFrom` premises of `env_id`. In this graph, `env_id` has no `InferredFrom` edges (it was asserted, not inferred). The contraction set is empty for `env_id` itself.

`deploy_target` is a causal descendant. It is NOT removed by contraction - only the belief being revised is contracted. The causal descendant's confidence will be updated by System 1.

**3c. Expansion**

The new value `"production"` is written to `environment`. K\*2 (success) satisfied: the new value is now in the belief set.

---

## Step 4: System 1 propagates confidence

After `revise()`, `propagate_system1(env_id)` runs automatically:

```
env_id.fast_confidence = 0.95   (new value, high confidence from tool result)

For each causal descendant of env_id:
    deploy_target:
        noisy_or = 1 - (1 - env.fast_confidence) = 0.95
        decay = exp(-1/ttl_ms * elapsed_ms) ~= 0.99  (recently inserted)
        deploy_target.fast_confidence = 0.95 * 0.99 ~= 0.941
```

State after Step 4:
- `environment`: 0.95 confidence, `"production"`
- `deploy_target`: 0.941 confidence, `"staging"` — value is stale; confidence rose because its causal antecedent (`environment`) was confirmed at 0.95 by a tool result. Noisy-OR propagates causal support, not semantic correctness. The high confidence here signals that `deploy_target` is well-supported by its premise — not that the value is right. The contract in Step 5 surfaces this divergence.

---

## Step 5: Contract check - deployment safety

```rust
use epica_contracts::{BehavioralContract, SessionInvariant};

let mut contract = BehavioralContract::new("deployment_safety", 0.05, 0.5);

// Precondition: environment must be known with at least 0.8 confidence
contract.add_precondition(
    "environment",
    BeliefExists { key: "environment" },
);
contract.add_precondition(
    "environment_confidence",
    MinConfidence { key: "environment", min: 0.8 },
);

// Invariant: deploy_target must match environment (conceptually)
// In practice: deploy_target must exist with >= 0.5 confidence
contract.add_invariant(SessionInvariant {
    key: "deploy_target".into(),
    min_confidence: 0.5,
    severity: ViolationClass::Hard,
});
```

**Precondition check**: `environment` exists at confidence 0.95 ≥ 0.8. Passes.

**Invariant check (as written)**: `deploy_target` exists at confidence 0.941 ≥ 0.5. Passes — the confidence floor is satisfied.

**What this invariant does not catch**: value equality. `deploy_target = "staging"` while `environment = "production"` is now a semantic inconsistency that the presence/confidence invariant cannot see. Adding an equality predicate to the invariant would fire here as a `Hard` violation and trigger `RecoveryPolicy::recover()` to revise `deploy_target` automatically. That equality predicate is omitted from this example to keep the Rust code minimal; Step 6 shows the manual revision path instead. This distinction — between what the contract is written to enforce and what it could enforce — is the reason the contract configuration is domain-specific and not inferred.

---

## Step 6: Agent revises deploy_target to match

```rust
quad.revise(
    target_id,
    BeliefValue::Inferred(serde_json::json!("production")),
    Provenance::LlmInference {
        model: "claude-sonnet-4-6".into(),
        call_id: uuid::Uuid::new_v4(),
        prompt_hash: 1,
    },
    0.88,
).unwrap();
```

System 1 propagates again. `deploy_target` is now `"production"` with confidence 0.88 (agent's inference) updated by Noisy-OR from `environment`'s 0.95.

---

## Step 7: Diff shows what changed

```rust
let diff = quad.diff_from_checkpoint(checkpoint_id);
println!("{:?}", diff);
// BeliefQuadDiff {
//   added: [],
//   removed: [],
//   modified: ["environment", "deploy_target"],
//   has_contradictions: true,
//   trajectory_ece: Some(0.06),
// }
```

`has_contradictions: true` because beliefs were modified. T-ECE for this session reflects the confidence trajectory.

---

## Step 8: Rollback (hypothetical)

If the tool result turns out to be wrong (a transient environment API glitch), the agent can roll back:

```rust
match quad.rollback_to(checkpoint_id) {
    Ok(diff) => {
        // Back to: environment = "staging" (0.90), deploy_target = "staging" (0.85)
        println!("Rolled back: {:?}", diff);
    }
    Err(RollbackError::UnnecessaryContraction(diff)) => {
        // K*4 guard: if no contradictions between current state and checkpoint,
        // rolling back would remove beliefs without cause - AGM violation
        println!("K*4 guard: rollback would be an unnecessary contraction");
    }
}
```

In this case, the rollback succeeds because `environment` and `deploy_target` changed values - the diff is non-empty, K\*4 does not block.

---

## Same scenario via the Python SDK

```python
from epica import BeliefQuad, BehavioralContract

quad = BeliefQuad()

# Insert beliefs
quad.insert("environment", "staging", 0.90, provenance="user")
quad.insert("deploy_target", "staging", 0.85, provenance="llm")
quad.add_causal_edge("environment", "deploy_target", effect_size=0.95)

# Checkpoint
cp = quad.checkpoint()

# Tool result: environment changed
quad.revise("environment", "production", 0.95, provenance="tool",
            tool_name="env_check")

# System 1 propagated automatically - check deploy_target confidence
node = quad.get("deploy_target")
print(f"deploy_target confidence after System 1: {node.fast_confidence:.3f}")
# -> 0.941 (Noisy-OR from environment's 0.95)

# Diff
diff = quad.diff_with_checkpoint(cp)
print(f"Modified: {diff.modified}")       # ["environment", "deploy_target"]
print(f"T-ECE: {diff.trajectory_ece}")

# Rollback
rolled = quad.rollback_to(cp)
print(f"Restored: {quad.get('environment').value}")  # "staging"
```

---

## Same scenario via the MCP server

```bash
# Start server
EPICA_NO_AUTH=1 cargo run --bin epica-serve

# Insert beliefs
curl -X POST http://localhost:8765/v1/beliefs \
     -H 'Content-Type: application/json' \
     -d '{"key":"environment","value":"staging","confidence":0.90}'

curl -X POST http://localhost:8765/v1/beliefs \
     -H 'Content-Type: application/json' \
     -d '{"key":"deploy_target","value":"staging","confidence":0.85}'

# Checkpoint
curl -X POST http://localhost:8765/v1/checkpoints
# -> {"checkpoint_id": "chk:..."}

# Tool result arrives
curl -X PATCH http://localhost:8765/v1/beliefs/environment \
     -H 'Content-Type: application/json' \
     -d '{"value":"production","confidence":0.95}'

# Check deploy_target (confidence propagated via System 1)
curl http://localhost:8765/v1/beliefs/deploy_target
# -> {"fast_confidence": 0.941, "value": "staging", ...}

# Diff from checkpoint
curl "http://localhost:8765/v1/diff?checkpoint_a=chk:..."
# -> {"modified": ["environment", "deploy_target"], "trajectory_ece": 0.06}

# Rollback
curl -X POST http://localhost:8765/v1/rollback \
     -H 'Content-Type: application/json' \
     -d '{"checkpoint_id": "chk:..."}'
```

---

## What this example demonstrates

| Capability | Where it fires |
|-----------|----------------|
| Belief insertion | Step 1 |
| Causal graph wiring | Step 1 (`add_causal_edge`) |
| Checkpoint | Step 2 |
| Contradiction detection | Step 3a |
| AGM contraction (minimal) | Step 3b |
| AGM expansion | Step 3c |
| System 1 Noisy-OR propagation | Step 4 |
| Contract precondition check | Step 5 |
| Contract invariant check | Step 5 |
| Belief diff with T-ECE | Step 7 |
| Rollback with K\*4 guard | Step 8 |
