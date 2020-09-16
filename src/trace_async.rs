// Copyright 2020 TiKV Project Authors. Licensed under Apache-2.0.

use std::cell::Cell;

struct AsyncTraceInner {
    collector: std::sync::Arc<crate::collector::CollectorInner>,
    next_suspending_state: Cell<crate::State>,
    suspending_begin_cycles: Cell<u64>,
    pending_event: u32,

    parent_id: u32,
    parent_begin_cycles: u64,
    parent_event: u32,
    parent_related_id: u32,
    parent_state: crate::State,
}

pub struct TraceHandle {
    inner: Option<AsyncTraceInner>,
}

pub struct LocalTraceGuard<'a> {
    // Option is for dropping.
    local: Option<crate::trace_local::LocalTraceGuard>,

    // `TraceHandle` may be used to trace a `Future` task which
    // consists of a sequence of local-tracings.
    //
    // We can treat the end of current local-tracing as the creation of
    // the next local-tracing. By the moment that the next local-tracing
    // is started, the gap time is the wait time of the next local-tracing.
    //
    // Here is the mutable reference for this purpose.
    handle: &'a AsyncTraceInner,
}

pub enum SettleGuard<'a> {
    TraceGuard(LocalTraceGuard<'a>),
    SpanGuard(crate::SpanGuard),
}

impl SettleGuard<'_> {
    pub(crate) fn take_local_guard(self) -> crate::trace_local::LocalGuard {
        match self {
            SettleGuard::TraceGuard(mut g) => {
                crate::trace_local::LocalGuard::TraceGuard(g.local.take().unwrap())
            }
            SettleGuard::SpanGuard(s) => crate::trace_local::LocalGuard::SpanGuard(s),
        }
    }
}

impl Drop for LocalTraceGuard<'_> {
    fn drop(&mut self) {
        drop(self.local.take());
        self.handle.suspending_begin_cycles.set(minstant::now());
    }
}

impl TraceHandle {
    pub(crate) fn new(parent_event: u32, pending_event: u32) -> Self {
        let trace_local = crate::trace_local::TRACE_LOCAL.with(|trace_local| trace_local.get());
        let tl = unsafe { &mut *trace_local };

        if tl.cur_collector.is_none() || tl.enter_stack.is_empty() {
            return Self { inner: None };
        }

        let collector = tl.cur_collector.as_ref().unwrap().clone();
        let related_id = *tl.enter_stack.last().unwrap();
        let parent_id = tl.next_id();
        let now_cycles = minstant::now();
        Self {
            inner: Some(AsyncTraceInner {
                collector,
                next_suspending_state: Cell::new(crate::State::Spawning),
                suspending_begin_cycles: Cell::new(now_cycles),
                pending_event,

                parent_id,
                parent_begin_cycles: now_cycles,
                parent_event,
                parent_related_id: related_id,
                parent_state: crate::State::Local,
            }),
        }
    }

    pub fn trace_enable<E: Into<u32>>(&self, event: E) -> Option<SettleGuard> {
        let settle_event = event.into();
        if let Some(inner) = &self.inner {
            let pending_event = inner.pending_event;

            let now_cycles = minstant::now();
            if let Some(local_guard) = crate::trace_local::LocalTraceGuard::new(
                inner.collector.clone(),
                now_cycles,
                crate::LeadingSpan {
                    // At this restoring time, fill this leading span the previously reserved suspending state,
                    // related id, begin cycles and ...
                    state: inner.next_suspending_state.get(),
                    related_id: inner.parent_id,
                    begin_cycles: inner.suspending_begin_cycles.get(),
                    // ... other fields calculating via them.
                    elapsed_cycles: now_cycles.wrapping_sub(inner.suspending_begin_cycles.get()),
                    event: pending_event,
                },
                settle_event,
            ) {
                // Reserve these for the next suspending process
                inner.next_suspending_state.set(crate::State::Scheduling);

                // Obviously, the begin cycles of the next suspending is impossible to predict, and it should
                // be recorded when `local_guard` is dropping. Here `LocalTraceGuard` is for this purpose.
                // See `impl Drop for LocalTraceGuard`.
                Some(SettleGuard::TraceGuard(LocalTraceGuard {
                    local: Some(local_guard),
                    handle: inner,
                }))
            } else {
                crate::new_span(settle_event).map(SettleGuard::SpanGuard)
            }
        } else {
            crate::new_span(settle_event).map(SettleGuard::SpanGuard)
        }
    }

    pub(crate) fn new_root(
        collector: std::sync::Arc<crate::collector::CollectorInner>,
        now_cycles: u64,
        parent_event: u32,
        pending_event: u32,
    ) -> Self {
        let trace_local = crate::trace_local::TRACE_LOCAL.with(|trace_local| trace_local.get());
        let tl = unsafe { &mut *trace_local };

        let root_id = tl.next_id();
        Self {
            inner: Some(AsyncTraceInner {
                collector,
                next_suspending_state: Cell::new(crate::State::Spawning),
                suspending_begin_cycles: Cell::new(now_cycles),
                pending_event,

                parent_id: root_id,
                parent_begin_cycles: now_cycles,
                parent_event,
                parent_related_id: 0,
                parent_state: crate::State::Root,
            }),
        }
    }
}

impl Drop for AsyncTraceInner {
    fn drop(&mut self) {
        if !self
            .collector
            .closed
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            self.collector.queue.push(crate::SpanSet {
                spans: vec![crate::Span {
                    id: self.parent_id,
                    state: self.parent_state,
                    related_id: self.parent_related_id,
                    begin_cycles: self.parent_begin_cycles,
                    elapsed_cycles: minstant::now().wrapping_sub(self.parent_begin_cycles),
                    event: self.parent_event,
                }],
                properties: crate::Properties::default(),
            });
        }
    }
}
