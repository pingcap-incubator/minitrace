// Copyright 2020 TiKV Project Authors. Licensed under Apache-2.0.

use crate::collector::Collector;
use crate::trace::*;
use crate::utils::{cycles_to_ns, real_time_ns};

/// Bind the current tracing context to another executing context.
///
/// ```
/// # use minitrace::thread::new_async_handle;
/// # use std::thread;
/// #
/// let mut handle = new_async_handle();
/// thread::spawn(move || {
///     let _g = handle.start_trace(0u32);
/// });
/// ```
#[inline]
pub fn new_async_handle() -> AsyncHandle {
    let trace = TRACE_LOCAL.with(|trace| trace.get());
    let tl = unsafe { &mut *trace };

    if tl.enter_stack.is_empty() {
        return AsyncHandle { inner: None };
    }

    let parent_id = *tl.enter_stack.last().unwrap();
    let inner = AsyncHandleInner {
        trace_id: tl.trace_id,
        collector: tl.collector.as_mut().map(|c| c.clone_into_box()),
        next_pending_parent_id: parent_id,
        begin_cycles: minstant::now(),
    };

    AsyncHandle { inner: Some(inner) }
}

struct AsyncHandleInner {
    trace_id: TraceId,
    collector: Option<Box<dyn Collector>>,
    next_pending_parent_id: SpanId,
    begin_cycles: u64,
}

#[must_use]
pub struct AsyncHandle {
    /// None indicates that tracing is not enabled
    inner: Option<AsyncHandleInner>,
}

impl AsyncHandle {
    pub fn start_trace<T: Into<u32>>(&mut self, event: T) -> Option<AsyncGuard<'_>> {
        let inner = self.inner.as_mut()?;

        let trace = TRACE_LOCAL.with(|trace| trace.get());
        let tl = unsafe { &mut *trace };

        let event = event.into();
        if !tl.enter_stack.is_empty() {
            Some(AsyncGuard::SpanGuard(Self::new_span(inner, event, tl)))
        } else {
            Some(AsyncGuard::AsyncScopeGuard(Self::new_scope(
                inner, event, tl,
            )?))
        }
    }

    #[inline]
    fn new_scope<'a>(
        handle_inner: &'a mut AsyncHandleInner,
        event: u32,
        tl: &mut TraceLocal,
    ) -> Option<AsyncScopeGuard<'a>> {
        let collector = handle_inner.collector.as_mut()?;
        if collector.is_closed() {
            handle_inner.collector = None;
            return None;
        }

        let now_cycle = minstant::now();
        let elapsed_cycles = now_cycle.wrapping_sub(handle_inner.begin_cycles);

        let pending_id = tl.new_span_id();
        let pending_span = Span {
            id: pending_id,
            state: State::Pending,
            parent_id: handle_inner.next_pending_parent_id,
            begin_cycles: handle_inner.begin_cycles,
            elapsed_cycles,
            event,
        };
        tl.spans.push(pending_span);

        let span_id = tl.new_span_id();
        let span_inner = SpanGuardInner::enter(
            Span {
                id: span_id,
                state: State::Normal,
                parent_id: pending_id,
                begin_cycles: now_cycle,
                elapsed_cycles: 0,
                event,
            },
            tl,
        );
        handle_inner.next_pending_parent_id = span_id;

        tl.trace_id = handle_inner.trace_id;
        tl.start_time_ns = real_time_ns().saturating_sub(cycles_to_ns(elapsed_cycles));
        tl.collector = Some(collector.clone_into_box());

        Some(AsyncScopeGuard {
            span_inner,
            handle_inner,
        })
    }

    #[inline]
    fn new_span(handle_inner: &mut AsyncHandleInner, event: u32, tl: &mut TraceLocal) -> SpanGuard {
        let parent_id = *tl.enter_stack.last().unwrap();
        let span_inner = SpanGuardInner::enter(
            Span {
                id: tl.new_span_id(),
                state: State::Normal,
                parent_id,
                begin_cycles: if handle_inner.begin_cycles != 0 {
                    handle_inner.begin_cycles
                } else {
                    minstant::now()
                },
                elapsed_cycles: 0,
                event,
            },
            tl,
        );
        handle_inner.begin_cycles = 0;

        SpanGuard { inner: span_inner }
    }
}

pub enum AsyncGuard<'a> {
    AsyncScopeGuard(AsyncScopeGuard<'a>),
    SpanGuard(SpanGuard),
}

pub struct AsyncScopeGuard<'a> {
    span_inner: SpanGuardInner,
    handle_inner: &'a mut AsyncHandleInner,
}

impl<'a> Drop for AsyncScopeGuard<'a> {
    #[inline]
    fn drop(&mut self) {
        let trace = TRACE_LOCAL.with(|trace| trace.get());
        let tl = unsafe { &mut *trace };

        let now_cycle = self.span_inner.exit(tl);
        self.handle_inner.begin_cycles = now_cycle;

        let (spans, properties) = tl.take_spans_and_properties();

        let mut c = tl.collector.take().unwrap();
        c.collect_span_set(SpanSet {
            trace_id: tl.trace_id,
            start_time_ns: tl.start_time_ns,
            cycles_per_second: minstant::cycles_per_second(),
            spans,
            properties,
        });

        tl.trace_id = 0;
        tl.start_time_ns = 0;
    }
}
