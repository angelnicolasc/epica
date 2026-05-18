#![no_main]
//! Fuzz target: deserializing arbitrary bytes as a `BeliefValue` must never panic.
//!
//! `BeliefValue` is part of Epica's public wire format. If a malformed message
//! reaches the deserializer (over MCP, from a persistence layer, or from a
//! user-supplied JSON file), the only acceptable outcome is `Err(_)` —
//! `panic!` would crash the server.

use libfuzzer_sys::fuzz_target;

use epica_core::BeliefValue;

fuzz_target!(|data: &[u8]| {
    // Attempt JSON deserialization. Any byte slice is accepted as input;
    // the only invariant is that the call returns instead of panicking.
    let _ = serde_json::from_slice::<BeliefValue>(data);
});
