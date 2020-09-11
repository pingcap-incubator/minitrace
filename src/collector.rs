// Copyright 2020 TiKV Project Authors. Licensed under Apache-2.0.

#[derive(Debug, Clone)]
pub(crate) struct SpanSet {
    /// Span collection
    pub spans: Vec<crate::Span>,

    /// Property collection
    pub properties: crate::Properties,
}

pub(crate) struct CollectorInner {
    pub(crate) queue: crossbeam::queue::SegQueue<SpanSet>,
    pub(crate) closed: std::sync::atomic::AtomicBool,
}

pub struct Collector {
    pub(crate) inner: std::sync::Arc<CollectorInner>,
    start_time_ns: u64,
    pub(crate) trace_handle: Option<crate::TraceHandle>,
}

impl Collector {
    pub(crate) fn new(start_time_ns: u64) -> Self {
        let collector = std::sync::Arc::new(crate::collector::CollectorInner {
            queue: crossbeam::queue::SegQueue::new(),
            closed: std::sync::atomic::AtomicBool::new(false),
        });

        crate::collector::Collector {
            inner: collector,
            start_time_ns,
            trace_handle: None,
        }
    }

    #[inline]
    pub fn collect(mut self) -> crate::TraceDetails {
        drop(self.trace_handle.take());
        let span_sets = self.collect_spanset();

        // Fast path to save memory allocation.
        if span_sets.len() == 1 {
            let span_set = span_sets.into_iter().next().unwrap();
            return crate::TraceDetails {
                start_time_ns: self.start_time_ns,
                elapsed_ns: crate::time::real_time_ns().saturating_sub(self.start_time_ns),
                cycles_per_second: minstant::cycles_per_second(),
                spans: span_set.spans,
                properties: span_set.properties,
            };
        }

        let mut spans_len = 0;
        let mut span_ids_len = 0;
        let mut property_lens_len = 0;
        let mut payload_len = 0;

        for span_set in &span_sets {
            spans_len += span_set.spans.len();
            span_ids_len += span_set.properties.span_ids.len();
            property_lens_len += span_set.properties.property_lens.len();
            payload_len += span_set.properties.payload.len();
        }

        let mut spans = Vec::with_capacity(spans_len);
        let mut span_ids = Vec::with_capacity(span_ids_len);
        let mut property_lens = Vec::with_capacity(property_lens_len);
        let mut payload = Vec::with_capacity(payload_len);

        for span_set in &span_sets {
            spans.extend_from_slice(&span_set.spans);
            span_ids.extend_from_slice(&span_set.properties.span_ids);
            property_lens.extend_from_slice(&span_set.properties.property_lens);
            payload.extend_from_slice(&span_set.properties.payload);
        }

        crate::TraceDetails {
            start_time_ns: self.start_time_ns,
            elapsed_ns: crate::time::real_time_ns().saturating_sub(self.start_time_ns),
            cycles_per_second: minstant::cycles_per_second(),
            spans,
            properties: crate::Properties {
                span_ids,
                property_lens,
                payload,
            },
        }
    }

    #[inline]
    fn collect_spanset(&self) -> Vec<SpanSet> {
        let len = self.inner.queue.len();
        let mut res = Vec::with_capacity(len);
        while let Ok(spans) = self.inner.queue.pop() {
            res.push(spans);
        }
        res
    }
}

impl Drop for Collector {
    fn drop(&mut self) {
        self.inner
            .closed
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
}
