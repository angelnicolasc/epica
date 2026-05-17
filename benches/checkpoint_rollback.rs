//! Benchmark: checkpoint + rollback round-trip.
//!
//! Target: < 10ms round-trip at 10K nodes.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use epica_core::{BeliefNode, BeliefQuad, BeliefValue, Provenance};

fn build_quad(n: usize) -> BeliefQuad {
    let mut quad = BeliefQuad::new();
    for i in 0..n {
        quad.insert(BeliefNode::new(
            &format!("belief_{i}"),
            BeliefValue::Deterministic(serde_json::json!(i)),
            Provenance::UserStatement { turn: i as u32 },
            0.9,
        ));
    }
    quad
}

fn bench_checkpoint_rollback(c: &mut Criterion) {
    let mut group = c.benchmark_group("checkpoint_rollback");

    for size in [100usize, 1_000, 10_000] {
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &n| {
            let base = build_quad(n);
            b.iter(|| {
                let mut quad = base.clone();
                let cp = quad.checkpoint();

                // Add a belief to make the rollback non-trivial (creates contradiction)
                quad.insert(BeliefNode::new(
                    "extra_belief",
                    BeliefValue::Deterministic(serde_json::json!("extra")),
                    Provenance::UserStatement { turn: 999 },
                    0.5,
                ));

                let _diff = quad.rollback_to(black_box(cp));
            });
        });
    }

    group.finish();
}

criterion_group!(benches, bench_checkpoint_rollback);
criterion_main!(benches);
