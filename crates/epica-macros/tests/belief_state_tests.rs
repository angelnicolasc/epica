// Integration tests for #[derive(BeliefState)].
//
// Each test defines its own small struct so attributes are fully isolated and
// failures point to the relevant attribute combination without interference.

use epica_macros::BeliefState;
use serde::{Deserialize, Serialize};

// ── Shared test types ──────────────────────────────────────────────────────────

#[derive(BeliefState, Debug, Default, Clone, Serialize, Deserialize)]
struct TwoFieldBeliefs {
    #[belief(source = "user")]
    intent: String,
    #[belief(confidence = 0.85)]
    suggestion: String,
}

#[derive(BeliefState, Debug, Default, Clone, Serialize, Deserialize)]
struct RoundTripBeliefs {
    // Default source = "llm:inference" → Inferred(Value::String(...)) → round-trip clean.
    #[belief(confidence = 0.7)]
    value: String,
}

#[derive(BeliefState, Debug, Default, Clone, Serialize, Deserialize)]
struct DefaultConfBeliefs {
    // Only source specified → confidence stays at default 0.5.
    #[belief(source = "llm:gpt4-turbo")]
    output: String,
}

#[derive(BeliefState, Debug, Default, Clone, Serialize, Deserialize)]
struct DecayBeliefs {
    #[belief(decays_after = "30s")]
    ephemeral: String,
}

#[derive(BeliefState, Debug, Default, Clone, Serialize, Deserialize)]
struct ProspectBeliefs {
    #[belief(prospect = true)]
    indexable: String,
}

#[derive(BeliefState, Debug, Default, Clone, Serialize, Deserialize)]
struct TauBeliefs {
    #[belief(reflection_threshold = 0.20)]
    belief_a: String,
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[test]
fn to_belief_quad_inserts_correct_count() {
    let b = TwoFieldBeliefs {
        intent: "refactor auth".into(),
        suggestion: "use jwt".into(),
    };
    let quad = b.to_belief_quad();
    assert_eq!(quad.iter().count(), 2);
}

#[test]
fn from_belief_quad_round_trips_string() {
    let original = RoundTripBeliefs { value: "round-trip-value".into() };
    let quad = original.to_belief_quad();
    let recovered = RoundTripBeliefs::from_belief_quad(&quad)
        .expect("from_belief_quad must return Some");
    assert_eq!(recovered.value, "round-trip-value");
}

#[test]
fn confidence_defaults_to_half() {
    let b = DefaultConfBeliefs { output: "hello".into() };
    let quad = b.to_belief_quad();
    let (fc, _sc) = DefaultConfBeliefs::output_confidence(&quad);
    assert!(
        (fc - 0.5_f32).abs() < f32::EPSILON,
        "expected 0.5, got {fc}"
    );
}

#[test]
fn typed_accessor_matches_generic() {
    let b = TwoFieldBeliefs {
        intent: "x".into(),
        suggestion: "y".into(),
    };
    let quad = b.to_belief_quad();
    let typed   = TwoFieldBeliefs::suggestion_confidence(&quad);
    let generic = TwoFieldBeliefs::field_confidence(&quad, "suggestion")
        .expect("field_confidence must return Some for known key");
    assert_eq!(typed, generic);
}

#[test]
fn unknown_key_returns_none() {
    let b = TwoFieldBeliefs::default();
    let quad = b.to_belief_quad();
    assert!(
        TwoFieldBeliefs::field_confidence(&quad, "nonexistent_key").is_none(),
        "unknown key must return None"
    );
}

#[test]
fn decays_after_30s_sets_ttl() {
    let b = DecayBeliefs { ephemeral: "short-lived".into() };
    let quad = b.to_belief_quad();
    let node = quad
        .iter()
        .find(|(_, n)| n.key == "ephemeral")
        .map(|(_, n)| n)
        .expect("ephemeral node must be present");
    assert_eq!(node.ttl_ms, Some(30_000));
}

#[test]
fn prospect_flag_set() {
    let b = ProspectBeliefs { indexable: "important-context".into() };
    let quad = b.to_belief_quad();
    let node = quad
        .iter()
        .find(|(_, n)| n.key == "indexable")
        .map(|(_, n)| n)
        .expect("indexable node must be present");
    assert!(node.prospect, "prospect flag must be true");
}

#[test]
fn reflection_threshold_override() {
    let b = TauBeliefs { belief_a: "some belief".into() };
    let quad = b.to_belief_quad();
    let node = quad
        .iter()
        .find(|(_, n)| n.key == "belief_a")
        .map(|(_, n)| n)
        .expect("belief_a node must be present");
    assert!(
        (node.reflection_threshold - 0.20_f32).abs() < 1e-6,
        "expected tau=0.20, got {}",
        node.reflection_threshold
    );
}
