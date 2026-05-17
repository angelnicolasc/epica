# Behavioral Contracts

Reference: Agent Behavioral Contracts (arXiv:2602.22302).

## What this document covers

How Epica's `C = (P, I, G, R)` contracts are structured, where they intervene in the runtime, what happens on violation, and the current enforcement scope.

## What is implemented today

`BehavioralContract` is fully implemented with precondition evaluation, invariant monitoring, governance policies, and recovery actions. Contracts are evaluated on every `update_belief()` call. TOML-based configuration via `ContractConfig` is implemented.

## What is still approximate or planned

- `(p, delta, k)`-satisfaction bounds are computed analytically (CLT-based); they are not empirically measured against a real agent trajectory.
- The ABC paper reports `5.2-6.8` soft violations per session as a baseline. This has not been measured on Epica's runtime against a real workload.
- Recovery actions are user-supplied; Epica does not verify their correctness.

---

## C = (P, I, G, R)

| Component | Type | When evaluated | What it does |
|-----------|------|----------------|--------------|
| P - Preconditions | `Vec<Box<dyn BeliefPredicate>>` | Before any `update_belief()` | Gate on current quad state; halt if unmet |
| I - Invariants | `Vec<SessionInvariant>` | Every `update_belief()` | Monitor ongoing belief state; classify violation |
| G - Governance | `GovernancePolicies` | On each write | Resource limits, token budgets, auth requirements |
| R - Recovery | `RecoveryPolicy` | On `Hard` or `Critical` violation | Automatic remediation or escalation |

---

## Where contracts intervene in the runtime

```text
BeliefRuntime::update_belief(key, new_value, confidence)
    |
    +-- [1] check_preconditions(quad)     <- P evaluated here
    |       Err -> ContractViolationError (halts update)
    |
    +-- [2] quad.revise()                 <- AGM revision
    |       System 1 propagation
    |
    +-- [3] check_governance(quad)        <- G evaluated here
    |       token budget, auth policy
    |
    +-- [4] System 2 (if threshold met)
    |
    \-- [5] check_invariants(quad)        <- I evaluated here
            Soft     -> log, continue
            Hard     -> RecoveryPolicy fires
            Critical -> halt, emit causal diff, escalate
```

---

## What happens on violation

| Class | Behavior | Recovery |
|-------|----------|----------|
| `Soft` | Logged to audit trail; `update_belief()` returns normally | None - monitoring only |
| `Hard` | `RecoveryPolicy` invoked immediately; update proceeds after recovery | User-supplied `RecoveryPolicy::recover()` |
| `Critical` | Runtime halts the update; emits `CausalDiff` of affected beliefs; returns `ContractViolationError` | Requires human escalation or rollback |

---

## Current enforcement scope

| Contract feature | Enforced | Enforcement point |
|------------------|----------|-------------------|
| Precondition key presence | Yes | `check_preconditions()` |
| Precondition min confidence | Yes | `check_preconditions()` |
| Invariant min confidence | Yes | `check_invariants()` |
| Invariant key presence | Yes | `check_invariants()` |
| Severity classification | Yes | `SessionInvariant.severity` field |
| Recovery policy execution | Yes | Hard violation path |
| Token budget governance | Yes | `GovernancePolicies.max_tokens` |
| Auth policy | Yes | `GovernancePolicies.auth` |
| Drift bound computation | Yes | `DriftBound::new(alpha, gamma)` |
| `(p, delta, k)`-satisfaction tracking | Computed (not empirically validated) | `drift_bound()` API |

---

## vs. AgentAssert

AgentAssert evaluates contracts over **output strings** - after the LLM has spoken. It operates at the text level.

Epica contracts operate over **belief mutations in Rust** - before the agent acts. They catch the "spiral of hallucination" (AUQ paper) at the epistemic level, not at the output level.

In a real deployment, AgentAssert can sit as a Python layer on top of `epica-mcp`: AgentAssert catches output-level violations; Epica catches belief-level violations. They are complementary, not alternatives.

---

## (p, delta, k)-satisfaction

A contract satisfies `(p, delta, k)` if, with probability `>= p`, no more than `delta` violations occur in any window of `k` steps.

Drift bound: `D* = alpha/gamma` where `alpha` is the agent's natural drift rate and `gamma` is the contract's enforcement rate. Computed via Central Limit Theorem concentration inequality.

```rust
let contract = BehavioralContract::new("deployment_safety", 0.05, 0.5);
// D* = 0.05 / 0.5 = 0.10
println!("Drift bound: {}", contract.drift_bound()); // 0.1
```

**Caveat**: `D*` is an analytical estimate, not an empirically measured value. Running Epica against a realistic agent workload and measuring actual violation rates would validate or invalidate this estimate. That measurement has not been performed.

---

## ViolationClass

| Class | Action |
|-------|--------|
| `Soft` | Log; continue |
| `Hard` | Activate `RecoveryPolicy` immediately |
| `Critical` | Halt + emit causal diff + human escalation |

## max_intervention_gap_steps

From StepShield (arXiv:2601.22136, 2026): the maximum steps between violation detection and intervention before damage becomes irreversible. Default: `1` (intervene on `Hard`/`Critical` immediately).

---

## Configuration via TOML

```toml
domain = "deployment_safety"
alpha  = 0.05
gamma  = 0.5

[[preconditions]]
type           = "min_confidence"
key            = "environment"
min_confidence = 0.8

[[invariants]]
key            = "user_goal"
min_confidence = 0.5
severity       = "hard"
```

Load with `EPICA_CONTRACTS_FILE=contracts/deployment.toml`.

---

## Current status

- Fully implemented in `crates/epica-contracts/`
- Tested: `cargo test -p epica-contracts`
- TOML config: `ContractConfig` in `crates/epica-contracts/src/config.rs`
- Integrated with MCP server via `EPICA_CONTRACTS_FILE`

## Known limitations

- `(p, delta, k)` drift bound is analytical only - not empirically validated
- ABC paper violation baseline (`5.2-6.8` soft/session) not reproduced on Epica's runtime
- Recovery policy correctness is not formally verified - user-supplied
