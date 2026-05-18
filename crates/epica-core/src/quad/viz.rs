//! Graphviz DOT serialization for [`BeliefQuad`].
//!
//! Renders both the causal and semantic projections in a single graph so
//! the viewer sees epistemic structure (causal lineage) and conceptual
//! structure (semantic relationships, including contradictions) in one
//! picture.
//!
//! ## Encoding
//!
//! - Nodes are coloured by `fast_confidence`:
//!   - `≥ 0.7` → `#52c41a` (green) — confident
//!   - `0.3 ..= 0.7` → `#faad14` (amber) — uncertain
//!   - `< 0.3` → `#ff4d4f` (red) — low confidence
//! - Causal `InferredFrom` edges: solid black arrow.
//! - Causal `Counterfactual` edges: dotted gray.
//! - Semantic `Contradicts` edges: dashed red, no arrowhead (symmetric).
//! - Other semantic edges: dashed gray.
//!
//! ## Rendering
//!
//! Write the DOT output to a file and pipe it through `dot`:
//!
//! ```bash
//! cargo run --example visualize_quad > out.dot
//! dot -Tsvg out.dot > out.svg
//! ```

use std::fmt::Write;

use crate::{
    belief::{BeliefId, BeliefValue},
    quad::{BeliefQuad, CausalEdge, SemanticEdge},
};

/// Serialize `quad` to a Graphviz DOT string.
///
/// The output is intended to be piped to `dot -Tsvg` or `dot -Tpng`. The
/// returned string is always a complete, parseable DOT document.
///
/// # Example
///
/// ```
/// use epica_core::{quad::viz::to_dot, BeliefNode, BeliefQuad, BeliefValue, Provenance};
///
/// let mut quad = BeliefQuad::new();
/// quad.insert(BeliefNode::new(
///     "intent", BeliefValue::Asserted("ship".into()),
///     Provenance::UserStatement { turn: 0 }, 0.9,
/// ));
/// let dot = to_dot(&quad);
/// assert!(dot.starts_with("digraph BeliefQuad"));
/// assert!(dot.contains("intent"));
/// ```
#[must_use]
pub fn to_dot(quad: &BeliefQuad) -> String {
    let mut out = String::with_capacity(1024);
    writeln!(out, "digraph BeliefQuad {{").unwrap();
    writeln!(out, "  rankdir = LR;").unwrap();
    writeln!(out, "  node [shape=box, style=filled, fontname=\"Helvetica\", fontsize=10];").unwrap();
    writeln!(out, "  edge [fontname=\"Helvetica\", fontsize=9];").unwrap();
    writeln!(out).unwrap();

    // Nodes — keyed by the SlotMap version of BeliefId so each iteration
    // produces a stable, unique identifier for DOT.
    for (id, node) in quad.iter() {
        let dot_id = node_dot_id(id);
        let label = format_label(&node.key, &node.value, node.fast_confidence);
        let color = confidence_color(node.fast_confidence);
        writeln!(
            out,
            "  {dot_id} [label=\"{label}\", fillcolor=\"{color}\"];"
        )
        .unwrap();
    }
    writeln!(out).unwrap();

    // Causal edges
    for (from, to, edge) in quad.causal().all_edges() {
        let (from_id, to_id) = (node_dot_id(from), node_dot_id(to));
        let (style, label) = causal_edge_style(&edge);
        writeln!(out, "  {from_id} -> {to_id} [{style}, label=\"{label}\"];").unwrap();
    }

    // Semantic edges (skip self-loops introduced by add_node default behaviour)
    for (from, to, edge) in quad.semantic().all_edges() {
        if from == to {
            continue;
        }
        let (from_id, to_id) = (node_dot_id(from), node_dot_id(to));
        let (style, label) = semantic_edge_style(&edge);
        writeln!(out, "  {from_id} -> {to_id} [{style}, label=\"{label}\"];").unwrap();
    }

    writeln!(out, "}}").unwrap();
    out
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn node_dot_id(id: BeliefId) -> String {
    // SlotMap keys serialise via Debug as something like `KeyData(1v1)`; we
    // want a DOT-safe identifier without parens or special chars.
    let raw = format!("{id:?}");
    let safe: String = raw
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect();
    format!("n_{safe}")
}

fn format_label(key: &str, value: &BeliefValue, confidence: f32) -> String {
    // Truncate long values to keep the graph readable.
    let value_repr = match value {
        BeliefValue::Asserted(s) => truncate(s, 24),
        other => truncate(&format!("{other:?}"), 24),
    };
    // Escape DOT-significant characters in the user-supplied strings.
    let key_escaped = escape_dot(key);
    let value_escaped = escape_dot(&value_repr);
    format!("{key_escaped}\\n{value_escaped}\\n conf={confidence:.2}")
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max).collect();
        format!("{cut}…")
    }
}

fn escape_dot(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn confidence_color(c: f32) -> &'static str {
    if c >= 0.7 {
        "#52c41a"
    } else if c >= 0.3 {
        "#faad14"
    } else {
        "#ff4d4f"
    }
}

fn causal_edge_style(edge: &CausalEdge) -> (&'static str, &'static str) {
    match edge {
        CausalEdge::Causes { .. } => ("color=black, penwidth=2", "causes"),
        CausalEdge::InferredFrom { .. } => ("color=black", "inferred"),
        CausalEdge::Counterfactual { .. } => ("color=gray, style=dotted", "counterfactual"),
    }
}

fn semantic_edge_style(edge: &SemanticEdge) -> (&'static str, &'static str) {
    match edge {
        SemanticEdge::Contradicts => ("color=red, style=dashed, arrowhead=none", "contradicts"),
        SemanticEdge::Subsumes => ("color=gray, style=dashed", "subsumes"),
        SemanticEdge::Synonymous => ("color=gray, style=dashed, arrowhead=none", "synonymous"),
    }
}
