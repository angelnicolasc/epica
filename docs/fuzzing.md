# Fuzzing

Epica ships two libFuzzer targets under `crates/epica-core/fuzz/` covering the
two highest-risk surfaces in the core crate:

| Target | What it fuzzes |
|---|---|
| `fuzz_revise` | `BeliefQuad::revise()` against arbitrary string mutations and out-of-range confidences. The invariant is: *no panic, ever, on any input*. |
| `fuzz_belief_value` | `serde_json::from_slice::<BeliefValue>(data)` against arbitrary byte slices. Validates the wire-format deserializer never panics on malformed input. |

Both targets are intentionally tight in scope — semantic correctness is the
proptest suite's job, not the fuzzer's. The fuzzer's job is to find inputs
that crash or hang the process.

## Running

cargo-fuzz requires **nightly Rust** (uses the unstable `-Z sanitizer` flag):

```bash
rustup install nightly
cargo install cargo-fuzz
```

Then, from the repository root:

```bash
cd crates/epica-core
cargo +nightly fuzz run fuzz_revise
cargo +nightly fuzz run fuzz_belief_value
```

Limit a run to N executions:

```bash
cargo +nightly fuzz run fuzz_revise -- -runs=1000000
```

Reproduce a previous crash from the artifacts directory:

```bash
cargo +nightly fuzz run fuzz_revise crates/epica-core/fuzz/artifacts/fuzz_revise/crash-<hash>
```

## CI policy

These targets are **not** wired into the main `ci.yml` workflow. libFuzzer
requires nightly toolchains and produces noisy artifacts; running it on every
push would burn runner minutes without proportional signal. The recommended
operating model is:

- **Local**: developers run the relevant target before touching `revise()` or
  the `BeliefValue` serde glue.
- **Scheduled**: a separate, opt-in workflow can be added under
  `.github/workflows/fuzz.yml` that runs each target for ~5 minutes on a
  weekly cron. Crashes get filed as issues automatically.

The scheduled workflow is intentionally not part of this sprint — adding it
without first letting the targets stabilise would generate noise on every
green main branch.
