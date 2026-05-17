use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use epica_core::{BeliefNode, BeliefQuad, BeliefValue, Provenance};

fn make_node(key: &str, confidence: f32) -> BeliefNode {
    BeliefNode::new(
        key,
        BeliefValue::Inferred(serde_json::json!(key)),
        Provenance::UserStatement { turn: 0 },
        confidence,
    )
}

fn bench_insert(c: &mut Criterion) {
    let mut group = c.benchmark_group("insert");
    for size in [100usize, 1_000, 10_000] {
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &n| {
            b.iter(|| {
                let mut quad = BeliefQuad::new();
                for i in 0..n {
                    quad.insert(make_node(&format!("belief_{i}"), 0.8));
                }
                black_box(quad.len())
            });
        });
    }
    group.finish();
}

fn bench_revise(c: &mut Criterion) {
    let mut group = c.benchmark_group("revise");
    for size in [100usize, 1_000, 10_000] {
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &n| {
            let mut quad = BeliefQuad::new();
            let mut ids = Vec::with_capacity(n);
            for i in 0..n {
                ids.push(quad.insert(make_node(&format!("b_{i}"), 0.8)));
            }

            b.iter(|| {
                for &id in &ids {
                    let _ = quad.revise(
                        id,
                        BeliefValue::Inferred(serde_json::json!("new")),
                        Provenance::UserStatement { turn: 1 },
                        0.9,
                    );
                }
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_insert, bench_revise);
criterion_main!(benches);
