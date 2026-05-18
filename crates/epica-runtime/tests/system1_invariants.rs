//! Property-based invariants for the System 1 fast-path.
//!
//! These tests do NOT exercise correctness of the Noisy-OR formula — that is
//! covered in `epica-core` unit tests. The goal here is broader: regardless of
//! input shape, after any sequence of admissible mutations the runtime must
//! never violate the structural invariants the rest of the system relies on.
//!
//! Invariants checked:
//!
//! - `fast_confidence ∈ [0.0, 1.0]` (never NaN, never out of range)
//! - `slow_confidence`, if present, satisfies the same bound
//! - `version` is strictly monotonic across mutations

use epica_core::{BeliefNode, BeliefQuad, BeliefValue, Provenance};
use epica_runtime::BeliefRuntime;
use proptest::prelude::*;

fn make_rt() -> BeliefRuntime {
    BeliefRuntime::new(BeliefQuad::new(), 0.5, 10, 0.0)
}

proptest! {
    /// After N arbitrary `update_belief` calls with random confidences (including
    /// out-of-range values that exercise the clamp path), every node in the quad
    /// has `fast_confidence ∈ [0.0, 1.0]` and no NaN.
    #[test]
    fn system1_confidence_stays_in_unit_interval(
        keys in proptest::collection::vec("[a-z]{3,8}", 1..8),
        confidences in proptest::collection::vec(-0.5f32..1.5f32, 1..32),
    ) {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(async {
            let runtime = make_rt();
            let mut ids = Vec::new();

            // Insert one belief per unique key (deduped to avoid implicit revision).
            let unique_keys: std::collections::BTreeSet<_> = keys.iter().collect();
            for (i, k) in unique_keys.iter().enumerate() {
                let unique = format!("k_{i}_{}", k);
                let id = runtime.insert_belief(BeliefNode::new(
                    &unique,
                    BeliefValue::Asserted("v".into()),
                    Provenance::UserStatement { turn: 0 },
                    0.5,
                )).await;
                ids.push(id);
            }

            // Drive random updates — confidence ranges INCLUDE out-of-range
            // values on purpose to exercise the clamp inside revise().
            for (turn, &conf) in confidences.iter().enumerate() {
                let id = ids[turn % ids.len()];
                let _ = runtime.update_belief(
                    id,
                    BeliefValue::Asserted(format!("v_{turn}")),
                    Provenance::UserStatement { turn: turn as u32 + 1 },
                    conf,
                ).await;
            }

            let quad = runtime.read_quad().await;
            for (_, node) in quad.iter() {
                prop_assert!(
                    node.fast_confidence.is_finite(),
                    "fast_confidence must be finite, got {}",
                    node.fast_confidence
                );
                prop_assert!(
                    (0.0..=1.0).contains(&node.fast_confidence),
                    "fast_confidence must be in [0, 1], got {}",
                    node.fast_confidence
                );
                if let Some(slow) = node.slow_confidence {
                    prop_assert!(
                        slow.is_finite() && (0.0..=1.0).contains(&slow),
                        "slow_confidence must be finite and in [0, 1], got {}",
                        slow
                    );
                }
            }
            Ok(())
        }).unwrap();
    }

    /// Version must be strictly monotonic across every successful mutation.
    /// A successful `insert_belief` followed by N successful `update_belief`
    /// must produce a quad whose `version()` is at least N + 1.
    #[test]
    fn quad_version_is_monotonic(
        update_count in 1usize..32,
    ) {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(async {
            let runtime = make_rt();
            let id = runtime.insert_belief(BeliefNode::new(
                "monotonic",
                BeliefValue::Asserted("v0".into()),
                Provenance::UserStatement { turn: 0 },
                0.5,
            )).await;

            let v_after_insert = runtime.read_quad().await.version();
            prop_assert!(v_after_insert >= 1, "insert must bump version");

            for i in 0..update_count {
                let before = runtime.read_quad().await.version();
                let _ = runtime.update_belief(
                    id,
                    BeliefValue::Asserted(format!("v{i}")),
                    Provenance::UserStatement { turn: i as u32 + 1 },
                    0.7,
                ).await;
                let after = runtime.read_quad().await.version();
                prop_assert!(
                    after > before,
                    "version must be strictly monotonic after a successful mutation: \
                     before={before}, after={after}"
                );
            }
            Ok(())
        }).unwrap();
    }
}
