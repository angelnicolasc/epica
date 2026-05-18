#![no_main]
//! Fuzz target: `BeliefQuad::revise()` must never panic on adversarial input.
//!
//! The harness builds a small quad with a known belief, then drives a revision
//! using `data` as the new asserted value (via `String::from_utf8_lossy`). The
//! invariant being checked is purely operational: no panics, no UB, no infinite
//! loops on any input shape — semantic correctness is covered by the proptest
//! suites in `tests/agm_postulates/`.

use libfuzzer_sys::fuzz_target;

use epica_core::{BeliefNode, BeliefQuad, BeliefValue, Provenance};

fuzz_target!(|data: &[u8]| {
    let mut quad = BeliefQuad::new();
    let id = quad.insert(BeliefNode::new(
        "fuzz_target",
        BeliefValue::Asserted("seed".to_string()),
        Provenance::UserStatement { turn: 0 },
        0.5,
    ));

    // Use a lossy conversion so any byte sequence is accepted as input.
    let new_str = String::from_utf8_lossy(data).into_owned();

    // The confidence is derived deterministically from the first byte so we
    // exercise the clamp path inside revise() for a wide range of values.
    let confidence = data.first().map(|b| (*b as f32 / 127.0) - 0.5).unwrap_or(0.5);

    let _ = quad.revise(
        id,
        BeliefValue::Asserted(new_str),
        Provenance::UserStatement { turn: 1 },
        confidence,
    );
});
