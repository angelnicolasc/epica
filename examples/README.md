# Examples

Each file under this directory is a runnable demonstration. They are
registered as `[[example]]` entries in the appropriate crate's `Cargo.toml`,
so the invocation always names the **crate** alongside the example.

| File | Crate | What it shows | Run |
|---|---|---|---|
| `minimal_revision.rs` | `epica-core` | Insert → revise → System 1 → diff → checkpoint → rollback → counterfactual. The smallest end-to-end demo of Phase 1. | `cargo run --example minimal_revision -p epica-core` |
| `codebase_agent.rs` | `epica-core` | Typed-belief agent over a synthetic codebase with rollback on contradiction. Mid-size, no LLM. | `cargo run --example codebase_agent -p epica-core` |
| `visualize_quad.rs` | `epica-core` | Build a sample quad and serialise it to Graphviz DOT. Pipe to `dot -Tsvg`. | `cargo run --example visualize_quad -p epica-core` |
| `multi_agent.rs` | `epica-runtime` | Two `BeliefRuntime` instances exchange beliefs via JSON serde — the wire format used for cross-process handoff. | `cargo run --example multi_agent -p epica-runtime` |
| `long_horizon.rs` | `epica-runtime` | 100-turn session with periodic checkpoints, selective rollback on contradiction, and T-ECE measurement. | `cargo run --example long_horizon -p epica-runtime` |
| `openai_provider.rs` | `epica-openai` | Full System 2 lifecycle with the OpenAI Chat Completions API as backend. Requires `OPENAI_API_KEY`. | `cargo run --example openai_provider -p epica-openai` |

All examples are excluded from the workspace's default test surface but
compile under `cargo check --workspace --all-targets`. If you add a new
example, register it in the relevant crate's `Cargo.toml` under a
`[[example]]` block and add a row here.
