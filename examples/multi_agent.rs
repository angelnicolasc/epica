//! Multi-agent belief portability via per-belief replay.
//!
//! Two `BeliefRuntime` instances stand in for two cooperating agents. Agent A
//! discovers facts about a project; agent B picks up where A left off by
//! replaying A's belief stream into a fresh quad. The example demonstrates:
//!
//!   - The supported handoff path for crossing a process boundary: extract
//!     each belief from A and replay it into B using the same API a normal
//!     consumer would.
//!   - Belief identity after transfer: agent B sees the same keys, values,
//!     provenance, and confidences as agent A wrote.
//!   - Continuing the session: agent B revises one of A's beliefs and the
//!     AGM postulates still hold across the boundary.
//!
//! ## Why per-belief replay, not full-quad serialisation
//!
//! `BeliefQuad` is `Serialize`-deriveable as a convenience but the internal
//! `HashMap<BeliefId, NodeIndex>` reverse-indices held by each of the four
//! graph projections do not round-trip through every serde format (JSON
//! requires string keys; postcard refuses certain serde shapes used by
//! `SlotMap`). Replaying the **public** belief stream is portable across
//! every serde backend the consumer cares to use because each `BeliefNode`
//! is plain data with no hidden invariants.
//!
//! Run with: `cargo run --example multi_agent -p epica-runtime`

use epica_core::{BeliefNode, BeliefQuad, BeliefValue, Provenance};
use epica_runtime::BeliefRuntime;

#[tokio::main]
async fn main() {
    // ── Agent A — investigates the codebase ──────────────────────────────────
    let agent_a = BeliefRuntime::new(BeliefQuad::new(), 0.5, 10, 1.0);

    agent_a
        .insert_belief(BeliefNode::new(
            "build_system",
            BeliefValue::Asserted("cargo workspace, 9 crates".into()),
            Provenance::ToolResult {
                tool: "fs.read".into(),
                call_id: uuid::Uuid::new_v4(),
            },
            0.95,
        ))
        .await;
    agent_a
        .insert_belief(BeliefNode::new(
            "primary_lang",
            BeliefValue::Asserted("rust 1.82+".into()),
            Provenance::ToolResult {
                tool: "fs.read".into(),
                call_id: uuid::Uuid::new_v4(),
            },
            0.97,
        ))
        .await;
    agent_a
        .insert_belief(BeliefNode::new(
            "feature_branch",
            BeliefValue::Asserted("main".into()),
            Provenance::UserStatement { turn: 0 },
            0.80,
        ))
        .await;

    // ── Extract A's beliefs as plain data — this is the wire format ──────────
    //
    // Each tuple round-trips through any serde backend the host chooses
    // (JSON, postcard, bincode, …). The MCP host would frame this list as
    // a single message; this example operates in-process.
    let handoff: Vec<BeliefNode> = {
        let q = agent_a.read_quad().await;
        q.iter().map(|(_, n)| n.clone()).collect()
    };
    let payload =
        serde_json::to_string(&handoff).expect("BeliefNode list round-trips via JSON");
    eprintln!(
        "agent A → handoff: {} bytes JSON, {} beliefs",
        payload.len(),
        handoff.len()
    );

    // ── Agent B — replays A's beliefs and extends them ───────────────────────
    let agent_b = BeliefRuntime::new(BeliefQuad::new(), 0.5, 10, 1.0);
    let inherited: Vec<BeliefNode> =
        serde_json::from_str(&payload).expect("inbound payload deserialises");
    for node in inherited {
        agent_b.insert_belief(node).await;
    }

    // B sees everything A wrote.
    let inherited_keys: Vec<String> = {
        let q = agent_b.read_quad().await;
        q.iter().map(|(_, n)| n.key.clone()).collect()
    };
    eprintln!("agent B inherited keys: {inherited_keys:?}");

    // B continues the session — adds a new belief.
    let migration_id = agent_b
        .insert_belief(BeliefNode::new(
            "migration_status",
            BeliefValue::Asserted("phase 5 complete".into()),
            Provenance::LlmInference {
                model: "claude-sonnet-4-6".into(),
                call_id: uuid::Uuid::new_v4(),
                prompt_hash: 0xc0_ffee,
            },
            0.78,
        ))
        .await;

    // B revises one of A's beliefs — branch changed.
    let branch_id = agent_b
        .get_by_key("feature_branch")
        .await
        .expect("feature_branch must survive the handoff");
    let result = agent_b
        .update_belief(
            branch_id,
            BeliefValue::Asserted("feat/multi-agent".into()),
            Provenance::UserStatement { turn: 1 },
            0.92,
        )
        .await
        .expect("AGM postulates hold across the boundary");

    eprintln!("agent B revised feature_branch → result: {result:?}");
    eprintln!(
        "agent B final state: {} beliefs (migration_status id = {migration_id:?})",
        agent_b.read_quad().await.iter().count()
    );
}
