// Copyright 2020 TiKV Project Authors. Licensed under Apache-2.0.

use super::Collector;
use crate::trace::SpanSet;
use crate::utils::real_time_ns;
use crate::{Properties, Span};
use crossbeam::channel::{unbounded, Receiver, Sender};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Debug, Clone, Default)]
pub struct TraceResult {
    pub trace_id: u64,
    pub start_time_ns: u64,
    pub cycles_per_second: u64,
    pub elapsed_ns: u64,
    pub spans: Vec<Span>,
    pub properties: Properties,
}

#[derive(Clone)]
pub struct SyncCollector {
    inner: Arc<CollectorInner>,
}

struct CollectorInner {
    is_closed: AtomicBool,
    tx: Sender<SpanSet>,
}

impl SyncCollector {
    pub fn new() -> (Arc<dyn Collector>, Finisher) {
        let (tx, rx) = unbounded();
        let inner = Arc::new(CollectorInner {
            is_closed: AtomicBool::new(false),
            tx,
        });
        let collector = Arc::new(SyncCollector {
            inner: inner.clone(),
        });
        let finisher = Finisher {
            inner,
            rx,
            start_time_ns: real_time_ns(),
        };
        (collector, finisher)
    }
}

impl Collector for SyncCollector {
    fn collect_span_set(&self, span_set: SpanSet) {
        let _ = self.inner.tx.try_send(span_set);
    }

    fn is_closed(&self) -> bool {
        self.inner.is_closed.load(Ordering::Relaxed)
    }
}

pub struct Finisher {
    inner: Arc<CollectorInner>,
    rx: Receiver<SpanSet>,
    start_time_ns: u64,
}

impl Finisher {
    pub fn finish(self) -> TraceResult {
        self.inner.is_closed.store(true, Ordering::Relaxed);
        let elapsed_ns = real_time_ns() - self.start_time_ns;

        let trace_id;
        let mut spans;
        let mut properties;

        if let Ok(SpanSet {
            trace_id: i,
            spans: s,
            properties: p,
            ..
        }) = self.rx.try_recv()
        {
            trace_id = i;
            spans = s;
            properties = p;
        } else {
            return TraceResult::default();
        }

        while let Ok(SpanSet {
            spans: s,
            properties:
                Properties {
                    span_ids,
                    property_lens,
                    payload,
                },
            ..
        }) = self.rx.try_recv()
        {
            spans.extend(&s);
            properties.span_ids.extend_from_slice(&span_ids);
            properties.property_lens.extend_from_slice(&property_lens);
            properties.payload.extend_from_slice(&payload);
        }

        TraceResult {
            trace_id,
            start_time_ns: self.start_time_ns,
            cycles_per_second: minstant::cycles_per_second(),
            elapsed_ns,
            spans,
            properties,
        }
    }
}
