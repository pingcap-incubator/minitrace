// Copyright 2020 TiKV Project Authors. Licensed under Apache-2.0.

use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn dummy_iter(i: usize) {
    #[minitrace::trace(0u32)]
    fn dummy() {}

    for _ in 0..i - 1 {
        dummy();
    }
}

#[minitrace::trace(0u32)]
fn dummy_rec(i: usize) {
    if i > 1 {
        dummy_rec(i - 1);
    }
}

fn trace_wide_bench(c: &mut Criterion) {
    c.bench_function_over_inputs(
        "trace_wide",
        |b, len| {
            b.iter(|| {
                let collector = {
                    let (_guard, collector) = minitrace::trace_enable(0u32);

                    if *len > 1 {
                        dummy_iter(*len);
                    }

                    collector
                };

                let _r = black_box(collector.collect());
            });
        },
        vec![1, 10, 100, 1000, 10000],
    );
}

fn trace_deep_bench(c: &mut Criterion) {
    c.bench_function_over_inputs(
        "trace_deep",
        |b, len| {
            b.iter(|| {
                let collector = {
                    let (_guard, collector) = minitrace::trace_enable(0u32);

                    if *len > 1 {
                        dummy_rec(*len);
                    }

                    collector
                };

                let _r = black_box(collector.collect());
            });
        },
        vec![1, 10, 100, 1000, 10000],
    );
}

fn trace_future_bench(c: &mut Criterion) {
    use minitrace::prelude::*;

    async fn f(i: u32) {
        for i in 0..i - 1 {
            async {}.trace_async(black_box(i)).await
        }
    }

    c.bench_function_over_inputs(
        "trace_future",
        |b, len| {
            b.iter(|| {
                let (collector, _) =
                    futures_03::executor::block_on(f(*len).future_trace_enable(0u32));

                black_box(collector.collect());
            });
        },
        vec![1, 10, 100, 1000, 10000],
    );
}

fn trace_start_context(c: &mut Criterion) {
    c.bench_function_over_inputs(
        "trace_context",
        |b, len| {
            b.iter(|| {
                let collector = {
                    let (_guard, collector) = minitrace::trace_enable(0u32);

                    for i in 0u32..len - 1 {
                        black_box(minitrace::trace_binder(i));
                    }

                    collector
                };

                black_box(collector.collect());
            });
        },
        vec![1, 10, 100, 1000, 10000],
    );
}

criterion_group!(
    benches,
    trace_wide_bench,
    trace_deep_bench,
    trace_future_bench,
    trace_start_context,
);
criterion_main!(benches);
