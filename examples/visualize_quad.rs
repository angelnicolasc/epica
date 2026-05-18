//! Render a small `BeliefQuad` to Graphviz DOT and print it to stdout.
//!
//! The output is a complete DOT document. Pipe to `dot` to produce SVG/PNG:
//!
//! ```bash
//! cargo run --example visualize_quad > out.dot
//! dot -Tsvg out.dot > out.svg     # or -Tpng for raster
//! ```
//!
//! The scenario constructed below is small but exercises every edge kind
//! the visualiser handles:
//!   - `InferredFrom` causal edges (forward inference)
//!   - `Counterfactual` causal edges
//!   - `Contradicts` semantic edges (drawn dashed red, no arrowhead)
//!   - `Subsumes` semantic edges (drawn dashed gray)
//!
//! Nodes are coloured by `fast_confidence`:
//!   - green ≥ 0.7, amber [0.3, 0.7), red < 0.3.

use epica_core::{
    quad::viz::to_dot, BeliefNode, BeliefQuad, BeliefValue, CausalEdge, Provenance, SemanticEdge,
};

fn main() {
    let mut quad = BeliefQuad::new();

    // Two premises with high confidence and one inferred conclusion that
    // depends on both. Best-evidence Noisy-OR will give the conclusion a
    // confidence close to the strongest premise.
    let premise_a = quad.insert(BeliefNode::new(
        "tool_result_a",
        BeliefValue::Asserted("file exists".into()),
        Provenance::ToolResult {
            tool: "fs.read".into(),
            call_id: uuid::Uuid::new_v4(),
        },
        0.92,
    ));
    let premise_b = quad.insert(BeliefNode::new(
        "tool_result_b",
        BeliefValue::Asserted("file is empty".into()),
        Provenance::ToolResult {
            tool: "fs.stat".into(),
            call_id: uuid::Uuid::new_v4(),
        },
        0.85,
    ));
    let conclusion = quad.insert(BeliefNode::new(
        "next_action",
        BeliefValue::Asserted("write new content".into()),
        Provenance::UserStatement { turn: 1 },
        0.40, // System 1 will rewrite this on propagation
    ));

    quad.add_causal_edge(
        premise_a,
        conclusion,
        CausalEdge::InferredFrom {
            premises: vec![premise_a, premise_b],
        },
    );
    quad.add_causal_edge(
        premise_b,
        conclusion,
        CausalEdge::InferredFrom {
            premises: vec![premise_a, premise_b],
        },
    );

    // A contradicting belief: someone asserted the opposite next action.
    let counterclaim = quad.insert(BeliefNode::new(
        "alt_action",
        BeliefValue::Asserted("ask user first".into()),
        Provenance::UserStatement { turn: 2 },
        0.55,
    ));
    quad.add_semantic_edge(conclusion, counterclaim, SemanticEdge::Contradicts);
    quad.add_semantic_edge(counterclaim, conclusion, SemanticEdge::Contradicts);

    // A low-confidence stale belief — drawn red to highlight the
    // confidence colour scale.
    let stale = quad.insert(BeliefNode::new(
        "user_mood",
        BeliefValue::Asserted("rushed".into()),
        Provenance::UserStatement { turn: 0 },
        0.15,
    ));
    quad.add_semantic_edge(stale, conclusion, SemanticEdge::Subsumes);

    // Run System 1 so the conclusion's confidence reflects its premises.
    quad.propagate_system1(premise_a);
    quad.propagate_system1(premise_b);

    print!("{}", to_dot(&quad));
}
