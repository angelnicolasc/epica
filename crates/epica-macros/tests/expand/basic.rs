// Trybuild pass test: two-field BeliefState with minimal attributes.
// Verifies that to_belief_quad, from_belief_quad, typed accessors,
// field_confidence, and schema_descriptor all compile.
use epica_macros::BeliefState;
use serde::{Deserialize, Serialize};

#[derive(BeliefState, Debug, Default, Clone, Serialize, Deserialize)]
pub struct BasicBeliefs {
    /// User-stated intent: stored as BeliefValue::Asserted.
    #[belief(source = "user")]
    pub intent: String,

    /// LLM-inferred suggestion with explicit confidence.
    #[belief(confidence = 0.85)]
    pub suggestion: String,
}

fn main() {
    let b = BasicBeliefs {
        intent: "refactor the auth module".into(),
        suggestion: "extract jwt validation into a crate".into(),
    };

    // to_belief_quad
    let quad = b.to_belief_quad();

    // from_belief_quad
    let _recovered = BasicBeliefs::from_belief_quad(&quad);

    // typed accessor (one per belief field)
    let _sug_conf: (f32, Option<f32>) = BasicBeliefs::suggestion_confidence(&quad);
    let _int_conf: (f32, Option<f32>) = BasicBeliefs::intent_confidence(&quad);

    // generic field_confidence
    let _generic: Option<(f32, Option<f32>)> = BasicBeliefs::field_confidence(&quad, "suggestion");

    // schema_descriptor
    let schema = BasicBeliefs::schema_descriptor();
    let _ = schema.beliefs.len();
}
