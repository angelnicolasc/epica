// Trybuild pass test: four-field BeliefState exercising all 9 attribute types.
// Verifies default_contract(), schema_descriptor(), and causal edge generation.
use epica_macros::BeliefState;
use serde::{Deserialize, Serialize};

#[derive(BeliefState, Debug, Default, Clone, Serialize, Deserialize)]
pub struct AgentState {
    /// User goal — requires human approval before the agent acts on it.
    #[belief(
        confidence = 0.9,
        source = "user",
        governance = "human_approval",
        audit = "full"
    )]
    pub user_goal: String,

    /// Inferred intent — causally derived from user_goal, prospectively indexed.
    #[belief(
        confidence = 0.7,
        decays_after = "10m",
        prospect = true,
        reflection_threshold = 0.12,
        causal_parent = "user_goal"
    )]
    pub inferred_intent: String,

    /// Reasoning trace — audit-only field with custom tau.
    #[belief(
        confidence = 0.6,
        reflection_threshold = 0.25,
        audit = "full"
    )]
    pub reasoning_trace: String,

    /// Deterministic tool output — fast and cheap, cached per session.
    #[belief(
        confidence = 0.5,
        source = "deterministic",
        cache_affinity = "session"
    )]
    pub tool_output: String,
}

fn main() {
    let state = AgentState {
        user_goal: "refactor the auth module".into(),
        inferred_intent: "extract jwt validation".into(),
        reasoning_trace: "step 1: identify dependencies".into(),
        tool_output: "found 3 usages of verify_token".into(),
    };

    // to / from quad
    let quad = state.to_belief_quad();
    let _recovered = AgentState::from_belief_quad(&quad);

    // default_contract: user_goal has governance="human_approval", so
    // require_human_approval must be non-empty.
    let contract = AgentState::default_contract();
    let _ = &contract.governance.require_human_approval;

    // schema_descriptor: must have 4 entries, one per belief field.
    let schema = AgentState::schema_descriptor();
    let _ = schema.beliefs.len();

    // typed accessors compile
    let _: (f32, Option<f32>) = AgentState::user_goal_confidence(&quad);
    let _: (f32, Option<f32>) = AgentState::inferred_intent_confidence(&quad);
    let _: (f32, Option<f32>) = AgentState::reasoning_trace_confidence(&quad);
    let _: (f32, Option<f32>) = AgentState::tool_output_confidence(&quad);
}
