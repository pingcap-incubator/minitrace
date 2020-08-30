// Copyright 2020 TiKV Project Authors. Licensed under Apache-2.0.

use criterion::{criterion_group, criterion_main, Criterion};

fn rustracing_harness() {
    fn dummy_rustracing(span: &rustracing::span::Span<()>) {
        for _ in 0..99 {
            let _child_span = span.child("child", |c| c.start_with_state(()));
        }
    }

    let (span_tx, span_rx) = crossbeam::channel::bounded(100);

    {
        let tracer = rustracing::Tracer::with_sender(rustracing::sampler::AllSampler, span_tx);
        let parent_span = tracer.span("parent").start_with_state(());
        dummy_rustracing(&parent_span);
    }

    let _r = span_rx.iter().collect::<Vec<_>>();
}

fn init_opentelemetry() {
    use opentelemetry::api::Provider;
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::prelude::*;
    use tracing_subscriber::Registry;

    let tracer = opentelemetry::sdk::Provider::default().get_tracer("component_name");
    let telemetry = tracing_opentelemetry::layer().with_tracer(tracer);

    Registry::default().with(telemetry).init();
}

fn opentelemetry_harness() {
    fn dummy_opentelementry() {
        for _ in 0..99 {
            let child = tracing::span!(tracing::Level::TRACE, "child");
            let _enter = child.enter();
        }
    }

    let root = tracing::span!(tracing::Level::TRACE, "parent");
    let _enter = root.enter();

    dummy_opentelementry();
}

fn minitrace_harness() {
    const PARENT: u32 = 0;
    const CHILD: u32 = 1;

    fn dummy_minitrace() {
        for _ in 0..99 {
            let _guard = minitrace::new_span(CHILD);
        }
    }

    let (collector, finisher) = minitrace::collector::SyncCollector::new();

    {
        let _root = minitrace::start_trace(0, PARENT, collector).unwrap();
        dummy_minitrace();
    }

    let _r = finisher.finish();
}

#[derive(Debug)]
enum TracingType {
    TokioTracing,
    Rustracing,
    Minitrace,
}

fn tracing_comparison(c: &mut Criterion) {
    init_opentelemetry();

    c.bench_function_over_inputs(
        "tracing_comparison",
        |b, tp| {
            b.iter(|| match tp {
                TracingType::TokioTracing => opentelemetry_harness(),
                TracingType::Rustracing => rustracing_harness(),
                TracingType::Minitrace => minitrace_harness(),
            });
        },
        &[
            TracingType::TokioTracing,
            TracingType::Rustracing,
            TracingType::Minitrace,
        ],
    );
}

criterion_group!(benches, tracing_comparison);
criterion_main!(benches);
