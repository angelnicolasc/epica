# AGM Belief Revision Postulates

Reference: Alchourron, Gardenfors, Makinson (1985); Kumiho (arXiv:2603.17244).

## What this document covers

Which AGM postulates Epica implements, which are approximated, and precisely where the approximation boundary lies. The honest answer to "does Epica satisfy AGM?" is: K\*2-K\*5 are satisfied as defined in the implementation; K\*6 is structurally approximated.

## What is implemented today

`PostulateAudit::verify()` computes all six postulates as pure functions before each mutation and attaches the result to `RevisionRecord`. Debug builds assert on violations; release builds record silently and continue.

`revise()` uses the Levi identity: contract `not phi` from `K`, then expand with `phi`.

## What is still approximate or planned

K\*6 compliance depends on semantic equivalence between belief values. Epica currently compares `BeliefValue` structurally (JSON equality). Two beliefs with identical meaning but different string representations are treated as non-equivalent ([TD-003](phase_roadmap.md#open-technical-debts)).

---

## Postulate map

| Postulate | Formal statement | Implementation | Compliance | Test |
|-----------|------------------|----------------|------------|------|
| K\*2 Success | `K*phi |- phi` | `revise()` always writes the new value | **Exact** | `k2_success.rs` |
| K\*3 Inclusion | `K*phi subseteq Cn(K + {phi})` | Expand-only path preserves all existing beliefs | **Exact** | `k3_inclusion.rs` |
| K\*4 Vacuity | `if not phi notin K then K subseteq K*phi` | `check_contradiction()` -> expand-only when no contradiction | **Exact** | `k4_vacuity.rs` |
| K\*5 Consistency | `K*phi != K_false if phi is consistent` | `is_self_contradictory(phi)` check before expansion | **Exact** | `k5_consistency.rs` |
| K\*6 Extensionality | `Cn({phi}) = Cn({psi}) -> K*phi = K*psi` | Structural equality of `BeliefValue` | **Approximate** | `k6_extensionality.rs` |

---

## Exact vs. approximate compliance

### What "exact" means here

For K\*2-K\*5: the implementation guarantees the postulate holds for every call to `revise()` given the definitions of contradiction and expansion used in the code. `PostulateAudit` verifies this and would panic in debug mode if a violation occurred.

It does **not** mean formal proof against an abstract AGM model. It means the test suite covers the postulate's stated condition and all tests pass.

### What "approximate" means for K\*6

K\*6 requires that logically equivalent inputs produce identical revision results. Epica's `check_contradiction()` compares `BeliefValue` via structural equality (`==` on `serde_json::Value`). Therefore:

- `"Paris is the capital of France"` and `"The capital of France is Paris"` are treated as **different** beliefs.
- Two calls to `revise()` with these two inputs will produce potentially different outcomes even though the beliefs are semantically equivalent.

This is an acceptable approximation for a runtime that does not have access to LLM semantics at the core level. The gap is tracked as TD-003.

### What would full K\*6 compliance require

Embedding-based similarity check in `check_contradiction()`: compute cosine similarity between `BeliefValue` embeddings and treat values above a threshold as equivalent. This requires an async LLM call in the hot path of `revise()` - a breaking API change deferred to a future phase.

---

## Levi identity

`revise(K, phi) = expand(contract(K, not phi), phi)`

In Epica:

1. `check_contradiction(id, new_value)` - is `new_value` inconsistent with current state?
2. If no: `expand_only()` (K\*4 vacuity: no contraction needed)
3. If yes: `minimal_contraction_set(id)` -> `apply_contraction()` -> set new value

---

## Minimal contraction

`minimal_contraction_set(id)` returns the direct premises of `id` via `inferred_from_premises(id)` - the beliefs that directly contributed to establishing the contradicted value via `InferredFrom` edges.

This satisfies **Hansson's Core-Retainment**: only beliefs that are part of the support of the contradicted belief are removed.

It does **not** guarantee Hansson-optimal minimality in belief sets with multiple independent causal paths to the same conclusion. If belief `B` can be supported by both `A` and `C` independently, and the contradiction involves `B`, both `A` and `C` are candidates for removal. The current implementation removes only the direct premises, which is conservative but not provably optimal in all graph topologies.

---

## PostulateAudit is an audit trail, not a gate

`PostulateAudit::verify()` runs **before** the mutation (capturing the pre-mutation state) and is attached to `RevisionRecord`. It does not block the mutation.

- **Debug builds**: `assert!` on K\*2, K\*3, K\*4, K\*5 violations
- **Release builds**: violation recorded silently; mutation proceeds

This is an explicit design decision: the audit trail is for observability, not enforcement. Enforcement happens through `BehavioralContract` invariants, not through the postulate checker.

If you want hard enforcement of AGM postulates in production, wire a `BehavioralContract` invariant that reads the `PostulateAudit` field from `RevisionRecord`.

---

## Rollback and K\*4

`rollback_to(checkpoint)` enforces K\*4 from the opposite direction: if the diff between the current state and the checkpoint has no contradictions, rolling back would unnecessarily contract beliefs - a K\*4 violation. Epica returns `Err(RollbackError::UnnecessaryContraction(diff))` in this case.

---

## Current status

- K\*2-K\*5: implemented and tested
- K\*6: approximated - structural equality only
- `PostulateAudit`: implemented in `crates/epica-core/src/revision/postulates.rs`
- Tests: `crates/epica-core/tests/agm_postulates/`

## Verification

```bash
cargo test -p epica-core
# Inspect: crates/epica-core/tests/agm_postulates/
```

## Known limitations

- K\*6 approximate ([TD-003](phase_roadmap.md#open-technical-debts))
- `PostulateAudit` is non-blocking in release builds
- Minimal contraction is conservative, not provably optimal in all graph topologies
