# AGM Belief Revision Postulates

Reference: Alchourrón, Gärdenfors, Makinson (1985); Kumiho
(arXiv:2603.17244); Hansson, "Kernel Contraction" (Core-Retainment).

## What this document covers

Which AGM postulates Epica implements, exactly how each is verified,
and where the only remaining approximation lives. The honest answer to
"does Epica satisfy AGM?" is: **K\*2–K\*6 are enforced as hard errors
in every build**. K\*6 is now a real semantic postulate when an
`EmbeddingProvider` is installed; without one, it falls back to the
structural equivalence used in Phase 1 — and the fallback is
indistinguishable from the pre-Sprint-1 behaviour, by design.

---

## Postulate map

| Postulate | Formal statement | Implementation | Compliance | Tests |
|---|---|---|---|---|
| K\*2 Success | `K*φ ⊨ φ` | `revise()` always writes the new value | **Exact + proptest 256+** | [`k2_success.rs`](../crates/epica-core/tests/agm_postulates/k2_success.rs) |
| K\*3 Inclusion | `K*φ ⊆ Cn(K + {φ})` | Expand-only path preserves all existing beliefs | **Exact + proptest 256+** | [`k3_inclusion.rs`](../crates/epica-core/tests/agm_postulates/k3_inclusion.rs) |
| K\*4 Vacuity | `if ¬φ ∉ K then K ⊆ K*φ` | `check_contradiction()` → expand-only when no contradiction | **Exact + proptest 256+** | [`k4_vacuity.rs`](../crates/epica-core/tests/agm_postulates/k4_vacuity.rs) |
| K\*5 Consistency | `K*φ ≠ K_⊥ if φ is consistent` | `is_self_contradictory(φ)` before expansion | **Exact + proptest 256+** | [`k5_consistency.rs`](../crates/epica-core/tests/agm_postulates/k5_consistency.rs) |
| **K\*6 Extensionality** | `Cn({φ}) = Cn({ψ}) ⟹ K*φ = K*ψ` | **Semantic via `EmbeddingProvider`** (structural fallback when cache cold) | **Exact, given the provider's verdict** | [`k6_extensionality.rs`](../crates/epica-core/tests/agm_postulates/k6_extensionality.rs) — 4 cases including paraphrase + anti-parallel + witness |

`PostulateAudit::verify()` computes all six postulates as pure
functions **before** each mutation. A violation of K\*2, K\*3, K\*5, or
K\*6 rejects the revision with
`BeliefRevisionError::PostulateViolation { postulate }`. K\*4 is
informational only — `vacuity = false` means a contraction is needed,
which is a legitimate outcome.

`revise()` uses the Levi identity: contract `¬φ` from `K`, then expand
with `φ`.

---

## K\*6 in detail: semantic equivalence via embeddings

### What it is

[`EmbeddingProvider`](../crates/epica-core/src/embedding/mod.rs) is a
sync trait whose hot-path method is `embed_cached(text) ->
Option<Vec<f32>>` — an in-memory cache lookup. Real embedding
computation (HTTP, ONNX, …) happens async via `warm_async` on the
concrete provider, populating the cache.

When the provider is installed via
`BeliefQuad::set_embedding_provider(...)`:

1. `SemanticGraph::value_contradicts_semantic` consults the cache for
   `Asserted(a)` vs `Asserted(b)` pairs.
2. If both texts are cached, cosine similarity classifies into
   `Equivalent(s)` (default `s ≥ 0.92`), `Contradicts(s)` (default
   `s ≤ −0.30`), or `Undecided(s)`.
3. `Equivalent` ⇒ no contradiction (paraphrase recognised).
   `Contradicts` ⇒ contradiction (treated as such even if strings
   happen to be similar). `Undecided` or cache miss ⇒ literal
   comparison fallback.
4. The verdict trace surfaces in
   `PostulateAudit::verdict_trace: VerdictTrace`. An in-quad paraphrase
   witness, if any, lives in
   `PostulateAudit::extensionality_witness: Option<BeliefId>`.

### Why "exact, given the provider's verdict"

K\*6 says "logically equivalent inputs produce identical revisions".
The implementation enforces this against whatever the
provider says about equivalence. If two beliefs are classified
`Equivalent` by the provider, the audit *demands* the revision
outcome match the outcome of revising with the other input — a
mismatch surfaces as `extensionality = false` with the offending peer
in `extensionality_witness`. The provider's classification *is* the
test of logical equivalence at this layer.

The fallback when no provider is installed reproduces the Phase 1
behaviour exactly — same code path, same JSON-equality comparison —
which is why every pre-Sprint-1 test in
`tests/agm_postulates/` continues to pass without changes.

### When this still falls short

The semantic path applies to `Asserted/Asserted` only. For
`BeliefValue::Inferred(JsonValue)` and `Deterministic(JsonValue)`, the
comparison stays structural (JSON equality). In practice this is OK
because those variants carry structured JSON where literal equality is
the correct test — but it's documented as **TD-P8-003** in the
DEVLOG.

### Default provider

`NullEmbeddingProvider` is the default in `BeliefQuad::new()`. Every
`embed_cached` returns `None`, so behaviour matches Phase 1
literal-only K\*6.

To enable real K\*6:

```rust
use std::sync::Arc;
use epica_core::{BeliefQuad, EmbeddingProvider};
use epica_openai::OpenAiEmbeddingProvider;

let provider: Arc<dyn EmbeddingProvider> =
    Arc::new(OpenAiEmbeddingProvider::from_env()?);
// (optional) pre-warm the cache asynchronously
provider.warm_async(&[
    "the user wants to refactor authentication",
    "user intent: refactor the auth subsystem",
]).await?;

let mut quad = BeliefQuad::new();
quad.set_embedding_provider(provider);
```

After `set_embedding_provider`, paraphrases of an `Asserted` belief
are recognised as equivalent — no AGM contraction, no false-positive
revision.

---

## Levi identity

`revise(K, φ) = expand(contract(K, ¬φ), φ)`

In Epica:

1. `check_contradiction(id, new_value)` — is `new_value` inconsistent
   with current state? (Consults `EmbeddingProvider` when one is
   installed; otherwise literal.)
2. If no: `expand_only()` (K\*4 vacuity — no contraction needed).
3. If yes: `minimal_contraction_set(id)` → `apply_contraction()` →
   set new value.

---

## Minimal contraction (Hansson Core-Retainment)

`minimal_contraction_set(id)` returns the direct premises of `id` via
`inferred_from_premises(id)` — the beliefs that directly contributed
to establishing the contradicted value via `InferredFrom` edges.

This satisfies **Hansson's Core-Retainment**: only beliefs that are
part of the support of the contradicted belief are candidates for
removal.

It does **not** guarantee Hansson-optimal minimality on belief sets
with multiple independent causal paths to the same conclusion. If
belief `B` is supported by both `A` and `C` independently and the
contradiction involves `B`, both `A` and `C` are candidates. The
current implementation removes only the direct premises — conservative
but not provably optimal in all graph topologies (documented as a
known limitation, low priority — real causal DAGs are shallow with
low fan-in).

---

## `PostulateAudit`: real critical gate (K\*2/3/5/6) + audit record (K\*4)

`PostulateAudit::verify()` runs before the mutation and is attached
to `RevisionRecord`. K\*2, K\*3, K\*5, K\*6 are **hard errors** — a
violation rejects the revision with `PostulateViolation`. K\*4 is
informational only.

Phase 1 marked the audit "non-blocking" — that has since changed:
K\*2/3/5/6 are critical and rejected. The decision lives in
[`epica-core/src/revision/agm.rs`](../crates/epica-core/src/revision/agm.rs):

```rust
let audit = PostulateAudit::verify(self, belief_id, &new_value, contradicts, trace);
if !audit.all_critical_pass() {
    return Err(BeliefRevisionError::PostulateViolation {
        postulate: audit.failed_postulate_name(),
    });
}
```

If hard enforcement at the *contract* layer is required on top of
this (e.g. "no revision ever may proceed if K\*6 audit is in
`LiteralDisagreeUndecided`"), a `BehavioralContract` invariant can
read `RevisionRecord.postulate_audit.verdict_trace` and reject.

---

## Rollback and K\*4

`rollback_to(checkpoint)` enforces K\*4 from the opposite direction:
if the diff between the current state and the checkpoint has no
contradictions, rolling back would unnecessarily contract beliefs —
a K\*4 violation. Epica returns
`Err(RollbackError::UnnecessaryContraction(diff))` in this case.

---

## Verification

```bash
# All AGM postulates including K*6 paraphrase + witness cases
cargo test -p epica-core --test agm_postulates

# End-to-end against the real OpenAI-compatible embedding provider
cargo test -p epica-openai --test embeddings        # 7 wiremock + 1 K*6 E2E
```

---

## Known limitations

- **K\*6 semantic path is `Asserted/Asserted` only** — TD-P8-003.
- **Minimal contraction is conservative**, not provably Hansson-optimal
  on multi-path graphs (low-priority known limitation).
- **No proptest for K\*6 semantic identities yet** — TD-P11-005
  (planned: random vector pairs with cosine constraints).
