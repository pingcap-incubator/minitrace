// Copyright 2020 TiKV Project Authors. Licensed under Apache-2.0.

use crossbeam_channel::Receiver;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::span::cycle::{Anchor, DefaultClock};
use crate::span::span_id::SpanId;
use crate::span::Span;
use crate::trace::acquirer::SpanCollection;

pub struct Collector {
    receiver: Receiver<SpanCollection>,
    closed: Arc<AtomicBool>,
}

impl Collector {
    /// Collects spans from traced routines.
    ///
    /// If passing `duration_threshold`, all spans will be reserved only when duration of the root
    /// span exceeds `duration_threshold`, otherwise only one span, the root span, will be returned.
    pub fn collect(
        self,
        need_sync: bool,
        duration_threshold: Option<Duration>,
        parent_id_of_root: Option<u64>,
    ) -> Vec<Span> {
        let span_collections: Vec<_> = if need_sync {
            self.receiver.iter().collect()
        } else {
            self.receiver.try_iter().collect()
        };
        self.closed.store(true, Ordering::SeqCst);

        let parent_id_of_root = parent_id_of_root.unwrap_or_default();
        let anchor = DefaultClock::anchor();
        if let Some(duration) = duration_threshold {
            // find the root span and check its duration
            if let Some(mut scope_span) = span_collections.iter().find_map(|s| match s {
                SpanCollection::ScopeSpan(s) if s.is_root() => Some(*s),
                _ => None,
            }) {
                scope_span.parent_id = SpanId::new(parent_id_of_root);
                let root_span = scope_span.build_span(anchor);
                if root_span.duration_ns < duration.as_nanos() as _ {
                    return vec![root_span];
                }
            }
        }

        Self::remove_unfinished_and_spawn_spans(span_collections, parent_id_of_root, anchor)
    }
}

impl Collector {
    #[inline]
    fn remove_unfinished_and_spawn_spans(
        span_collections: Vec<SpanCollection>,
        parent_id_of_root: u64,
        anchor: Anchor,
    ) -> Vec<Span> {
        let capacity = span_collections
            .iter()
            .map(|sc| match sc {
                SpanCollection::LocalSpans { spans, .. } => spans.len(),
                SpanCollection::ScopeSpan(_) => 1,
            })
            .sum();

        let mut spans = Vec::with_capacity(capacity);
        let mut pending_scope_spans = Vec::with_capacity(span_collections.len());
        let mut parent_ids_of_spawn_spans = HashMap::with_capacity(span_collections.len());

        for span_collection in span_collections {
            match span_collection {
                SpanCollection::LocalSpans {
                    spans: local_spans,
                    parent_span_id,
                } => {
                    let mut remaining_descendant_count = 0;
                    for span in &*local_spans {
                        if remaining_descendant_count > 0 {
                            remaining_descendant_count -= 1;
                            if span._is_spawn_span {
                                parent_ids_of_spawn_spans.insert(span.id, span.parent_id);
                                continue;
                            }

                            spans.push(span.build_span(anchor));
                        } else if span.end_cycle.is_zero() {
                            // remove unfinished span
                            continue;
                        } else {
                            if span._is_spawn_span {
                                parent_ids_of_spawn_spans.insert(span.id, parent_span_id);
                                continue;
                            }

                            let mut span = span.clone();
                            span.parent_id = parent_span_id;
                            remaining_descendant_count = span._descendant_count;
                            spans.push(span.build_span(anchor));
                        }
                    }
                }
                SpanCollection::ScopeSpan(mut scope_span) => {
                    if scope_span.is_root() {
                        scope_span.parent_id = SpanId::new(parent_id_of_root);
                        spans.push(scope_span.build_span(anchor));
                    } else {
                        pending_scope_spans.push(scope_span);
                    }
                }
            }
        }

        for mut span in pending_scope_spans {
            if let Some(parent_id) = parent_ids_of_spawn_spans.get(&span.parent_id) {
                span.parent_id = *parent_id;
            }
            spans.push(span.build_span(anchor));
        }

        spans
    }
}

impl Collector {
    pub(crate) fn new(receiver: Receiver<SpanCollection>, closed: Arc<AtomicBool>) -> Self {
        Collector { receiver, closed }
    }
}
