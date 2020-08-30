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
                let (collector, finisher) = minitrace::collector::SyncCollector::new();

                {
                    let _root = minitrace::start_trace(0, 0u32, collector).unwrap();
                    if *len > 1 {
                        dummy_iter(*len);
                    }
                }

                finisher.finish();
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
                let (collector, finisher) = minitrace::collector::SyncCollector::new();

                {
                    let _root = minitrace::start_trace(0, 0u32, collector).unwrap();
                    if *len > 1 {
                        dummy_rec(*len);
                    }
                }

                finisher.finish();
            });
        },
        vec![1, 10, 100, 1000, 10000],
    );
}

fn trace_future_bench(c: &mut Criterion) {
    use minitrace::future::FutureExt;

    async fn f(i: u32) {
        for i in 0..i - 1 {
            async {}.in_new_span(black_box(i)).await
        }
    }

    c.bench_function_over_inputs(
        "trace_future",
        |b, len| {
            b.iter(|| {
                let (collector, finisher) = minitrace::collector::SyncCollector::new();

                {
                    let _root = minitrace::start_trace(0, 0u32, collector).unwrap();
                    let _ = futures_03::executor::block_on(f(*len).in_new_span(0u32));
                }

                finisher.finish();
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
                let (collector, finisher) = minitrace::collector::SyncCollector::new();

                {
                    let _root = minitrace::start_trace(0, 0u32, collector).unwrap();
                    for _ in 0..*len {
                        let _guard = black_box(minitrace::thread::new_async_handle());
                    }
                }

                finisher.finish();
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
    trace_start_context
);
criterion_main!(benches);
