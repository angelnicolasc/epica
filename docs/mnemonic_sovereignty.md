# Mnemonic Sovereignty

Reference: Mnemonic Sovereignty survey (arXiv:2604.16548).

## What this document covers

How Epica implements the nine memory governance primitives from the Mnemonic Sovereignty survey, what "verifiable deletion" means in this runtime, and the enforcement fidelity per primitive.

## What is implemented today

All nine primitives are enforced at runtime. `MnemonicSovereignty` is constructed with all nine policy structs and passed to `BeliefRuntime` at construction; it gates every `update_belief()`, `retrieve_for_query()`, and `rollback_to()` call, and every `LongTermMemoryStore` persistence operation.

Enforcement fidelity per primitive:
- **Primitives 1-4, 6, 8-9** - enforced exactly: violations return `Err` or halt the operation.
- **Primitive 5 (forget)** - exhaustive traversal across all four `BeliefQuad` graphs after erasure; confirms no reachable reference. Not a cryptographic erasure proof (see *Forget policy*).
- **Primitive 7 (cross-agent propagation)** - enforced at the MCP server boundary; cryptographic verification between agent instances is not implemented.

## What is still approximate or planned

- Primitive 7 cryptographic cross-agent verification is not implemented; cross-agent policy relies on the MCP server as a trust boundary. Agents communicating outside the MCP layer bypass this enforcement.
- `RecoveryVerifier` correctness is not formally checked; the callback is user-supplied.
- Full cryptographic erasure (zero-knowledge proofs, Merkle audit logs, hardware-attested deletion) remains an open research problem per the survey; Epica's forget policy is the strongest in-process approximation achievable without cryptographic infrastructure.

---

## The nine primitives

| # | Primitive | Struct | Enforced where | Implemented? |
|---|-----------|--------|----------------|--------------|
| 1 | Write authorization | `AuthPolicy` | `update_belief()` precondition | Yes |
| 2 | Read authorization | `AuthPolicy` | `retrieve_for_query()` filter | Yes |
| 3 | Update authorization | `AuthPolicy` | `update_belief()` precondition | Yes |
| 4 | Retention policy | `RetentionPolicy` | `LongTermMemoryStore::flush()` | Yes - Redis TTL enforced |
| 5 | Forget policy | `ForgetPolicy` | `update_belief()` + graph traversal | Yes - exhaustive traversal (see below) |
| 6 | Audit trail | `AuditPolicy` | Every `update_belief()` call | Yes - configurable mode (full / selective / violations-only) |
| 7 | Cross-agent propagation | `CrossAgentPolicy` | MCP server boundary | Yes - gated; cross-agent cryptographic verification not implemented |
| 8 | Rollback authorization | `AuthPolicy` | `rollback_to()` precondition | Yes |
| 9 | Recovery verification | `RecoveryVerifier` | `rollback_to()` post-condition | Yes - user-supplied verification callback |

---

## Forget policy

Forget is distinct from delete. `ForgetPolicy` includes a `verify_fn` that, after erasure, traverses all four graphs of the `BeliefQuad` to confirm that the target belief is not reachable via any path in any of the four graphs (semantic, temporal, causal, entity).

### What "verifiable deletion" means here

After `ForgetPolicy` executes erasure, the verifier traverses:
1. `SemanticGraph` - checks no node or edge references the erased `BeliefId`
2. `TemporalGraph` - same
3. `CausalGraph` - same, including transitive reachability check
4. `EntityGraph` - same

If the `BeliefId` appears in any graph after traversal, the forget operation returns an error.

### What it does not mean

- It is **not** a cryptographic proof of erasure. There is no zero-knowledge proof, Merkle audit log, or hardware-attested deletion.
- It does **not** guarantee deletion from the `LongTermMemoryStore` (Redis). The Redis key expires according to `RetentionPolicy` TTL; `flush()` does not perform a verified-erasure traversal.
- It does **not** cover backups, checkpoints serialized to disk, or cross-agent belief copies.

The survey identifies verifiable deletion as an open research problem. Epica's implementation is a runtime-level approximation: exhaustive in-process traversal that detects accidental retention, not a formally proven erasure guarantee.

---

## Enforcement

`MnemonicSovereignty` is constructed with all nine policies and passed to `BeliefRuntime`:

```rust
let sovereignty = MnemonicSovereignty::builder()
    .write_auth(AuthPolicy::require_role("agent"))
    .forget(ForgetPolicy::with_verify())
    .retention(RetentionPolicy::ttl_ms(86_400_000))
    .build();

let runtime = BeliefRuntime::new(quad, 0.15, 50, 1.0)
    .with_sovereignty(sovereignty);
```

Every call to `update_belief()` evaluates write authorization, audit logging, and invariant checks before any mutation. `rollback_to()` evaluates rollback authorization and recovery verification after restoration.

---

## Known limitations

- Cross-agent policy enforcement requires the MCP server to act as a trust boundary. Agents communicating outside the MCP layer bypass this enforcement.
- `AuditPolicy::SelectiveMode` logs only beliefs matching a user-supplied predicate. Gaps in the predicate create audit gaps.
- `AuditLedger` is in-memory only; the Merkle chain does not survive process restarts. `SledTaskStore` covers MCP task durability (TD-P5-002 closed) but not ledger persistence (TD-P9-001).
- `PostulateAudit` (AGM audit trail) is separate from the sovereignty audit trail; they are not correlated in the current implementation.

---

## Current status

- All 9 primitives: implemented in `crates/epica-contracts/src/sovereignty.rs`
- Tested: via `cargo test -p epica-contracts`
- Forget verification: exhaustive graph traversal - confirmed by integration tests
- Known limitation: not cryptographic; not persistent across Redis flush failures

## Next validation step

Run `cargo test -p epica-contracts` and inspect the forget-policy tests to confirm traversal behavior. Test a Redis flush with an intentionally retained key to verify the gap described above.
