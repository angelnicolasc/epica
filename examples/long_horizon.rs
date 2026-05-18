//! Long-horizon agent session: 100 turns with periodic checkpoints and
//! a roll-back on contradiction.
//!
//! Demonstrates:
//!   - Sessions with many beliefs where individual updates are cheap
//!     (per-belief cost stays sub-millisecond, see BENCHMARKS.md).
//!   - Checkpointing every 10 turns so an undesirable contradiction
//!     can be undone without losing the whole session.
//!   - T-ECE measurement at the end so the calibration of the agent's
//!     confidences is quantifiable.
//!
//! Run with: `cargo run --example long_horizon`

use epica_core::{BeliefNode, BeliefQuad, BeliefValue, Provenance};
use epica_runtime::BeliefRuntime;

const SESSION_LENGTH: usize = 100;
const CHECKPOINT_EVERY: usize = 10;

#[tokio::main]
async fn main() {
    let rt = BeliefRuntime::new(BeliefQuad::new(), 0.5, 50, 1.0);
    let mut checkpoint_ids = Vec::new();

    // ── Drive 100 turns. Most turns add a belief; some revise prior ones. ────
    let mut last_belief_ids = Vec::new();
    for turn in 0..SESSION_LENGTH {
        if turn % 7 == 6 && !last_belief_ids.is_empty() {
            // Every 7th turn, the agent revises a recently created belief.
            // Confidence drifts to simulate updated evidence.
            let target = last_belief_ids[last_belief_ids.len() - 1];
            let _ = rt
                .update_belief(
                    target,
                    BeliefValue::Asserted(format!("revised at turn {turn}")),
                    Provenance::UserStatement { turn: turn as u32 },
                    0.7 + ((turn % 5) as f32) * 0.05,
                )
                .await;
        } else {
            // Normal turn: write a new tool observation.
            let id = rt
                .insert_belief(BeliefNode::new(
                    format!("observation_{turn:03}"),
                    BeliefValue::Asserted(format!("event payload {turn}")),
                    Provenance::ToolResult {
                        tool: "monitor.tick".into(),
                        call_id: uuid::Uuid::new_v4(),
                    },
                    // Confidence varies a little so T-ECE has signal at the end.
                    0.6 + ((turn % 9) as f32) * 0.04,
                ))
                .await;
            last_belief_ids.push(id);
        }

        if turn > 0 && turn % CHECKPOINT_EVERY == 0 {
            let cp = rt.checkpoint().await;
            checkpoint_ids.push(cp);
        }
    }

    eprintln!(
        "session completed: {} beliefs after {} turns, {} checkpoints captured",
        rt.read_quad().await.iter().count(),
        SESSION_LENGTH,
        checkpoint_ids.len()
    );

    // ── Demonstrate selective rollback ───────────────────────────────────────
    // Suppose the last 10 turns introduced a contradiction. Roll back to the
    // checkpoint taken 10 turns ago and continue from there.
    if let Some(&last_safe) = checkpoint_ids.last() {
        let before = rt.read_quad().await.iter().count();
        rt.rollback_to(last_safe)
            .await
            .expect("rollback to most recent checkpoint succeeds");
        let after = rt.read_quad().await.iter().count();
        eprintln!(
            "rolled back to last checkpoint: {before} → {after} beliefs ({} discarded)",
            before - after
        );
    }

    // ── Finalize and report T-ECE ────────────────────────────────────────────
    rt.finalize_session().await;
    match rt.compute_tece().await {
        Some(tece) => eprintln!(
            "session T-ECE = {tece:.4}  (target: < 0.08 for well-calibrated agents)"
        ),
        None => eprintln!("no T-ECE available — no confidence outcomes were recorded"),
    }
}
