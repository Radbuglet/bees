use std::hint::black_box;

use bees::Allocation;
use criterion::{criterion_group, criterion_main, Criterion};

fn criterion_benchmark(c: &mut Criterion) {
    let alloc = Allocation::new(1);
    let object = alloc.put(0, 4u32);

    c.bench_function("is alive", |b| b.iter(|| black_box(object).is_alive()));

    c.bench_function("read value", |b| b.iter(|| black_box(object).read()));
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
