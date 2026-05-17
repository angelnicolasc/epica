//! Benchmark: System 1 propagation latency vs. flat HashMap confidence lookup.
//!
//! Target (playbook): < 2× overhead vs. HashMap at 10K nodes in a chain graph.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use epica_core::{BeliefNode, BeliefQuad, BeliefValue, CausalEdge, Provenance};
use std::collections::HashMap;

fn build_chain_quad(n: usize) -> (BeliefQuad, epica_core::BeliefId) {
    let mut quad = BeliefQuad::new();
    let mut prev_id = None;
    let mut first_id = None;

    for i in 0..n {
        let id = quad.insert(BeliefNode::new(
            &format!("node_{i}"),
            BeliefValue::Inferred(serde_json::json!(i)),
            Provenance::UserStatement { turn: i as u32 },
            0.9,
        ));
        if i == 0 {
            first_id = Some(id);
        }
        if let Some(prev) = prev_id {
            quad.add_causal_edge(prev, id, CausalEdge::InferredFrom { premises: vec![prev] });
        }
        prev_id = Some(id);
    }

    (quad, first_id.unwrap())
}

fn bench_system1(c: &mut Criterion) {
    let mut group = c.benchmark_group("system1_propagation");

    for size in [100usize, 1_000, 10_000] {
        group.bench_with_input(
            BenchmarkId::new("chain_quad", size),
            &size,
            |b, &n| {
                let (mut quad, root) = build_chain_quad(n);
                b.iter(|| {
                    quad.propagate_system1(black_box(root));
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("hashmap_baseline", size),
            &size,
            |b, &n| {
                let mut map: HashMap<usize, f32> = (0..n).map(|i| (i, 0.9f32)).collect();
                b.iter(|| {
                    for v in map.values_mut() {
                        *v = black_box(0.9 * 1.0);
                    }
                });
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_system1);
criterion_main!(benches);
