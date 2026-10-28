#![forbid(unsafe_code)]
//! Placeholder for future pure mechanism benchmarks.

use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;

fn placeholder(c: &mut Criterion) {
    c.bench_function("placeholder", |b| {
        b.iter(|| black_box(1_u64) + black_box(2));
    });
}

criterion_group!(benches, placeholder);
criterion_main!(benches);
