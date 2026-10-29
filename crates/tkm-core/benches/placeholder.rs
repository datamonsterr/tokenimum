#![forbid(unsafe_code)]
//! Placeholder for future pure mechanism benchmarks.

use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;

fn placeholder(c: &mut Criterion) {
    // Sub-nanosecond work rounds to 0 or 1 ns and makes the 20% gate flip between runs.
    c.bench_function("placeholder_sum_1k", |b| {
        b.iter(|| (0..black_box(1_000_u64)).map(black_box).sum::<u64>());
    });
}

criterion_group!(benches, placeholder);
criterion_main!(benches);
