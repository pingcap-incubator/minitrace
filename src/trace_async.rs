// Copyright 2020 TiKV Project Authors. Licensed under Apache-2.0.

struct AsyncTraceInner {
    collector: std::sync::Arc<crate::collector::CollectorInner>,
    next_suspending_state: crate::State,
    next_related_id: u32,
    suspending_begin_cycles: u64,
    pending_event: Option<u32>,
}

pub struct TraceHandle {
    inner: Option<AsyncTraceInner>,
}

pub struct LocalTraceGuard<'a> {
    _local: crate::trace_local::LocalTraceGuard,

    // `TraceHandle` may be used to trace a `Future` task which
    // consists of a sequence of local-tracings.
    //
    // We can treat the end of current local-tracing as the creation of
    // the next local-tracing. By the moment that the next local-tracing
    // is started, the gap time is the wait time of the next local-tracing.
    //
    // Here is the mutable reference for this purpose.
    handle: &'a mut AsyncTraceInner,
}

impl Drop for LocalTraceGuard<'_> {
    fn drop(&mut self) {
        self.handle.suspending_begin_cycles = minstant::now();
    }
}

impl TraceHandle {
    pub(crate) fn new(pending_event: Option<u32>) -> Self {
        let trace_local = crate::trace_local::TRACE_LOCAL.with(|trace_local| trace_local.get());
        let tl = unsafe { &mut *trace_local };

        if tl.cur_collector.is_none() || tl.enter_stack.is_empty() {
            return Self { inner: None };
        }

        let collector = tl.cur_collector.as_ref().unwrap().clone();
        let related_id = *tl.enter_stack.last().unwrap();
        Self {
            inner: Some(AsyncTraceInner {
                collector,
                next_suspending_state: crate::State::Spawning,
                next_related_id: related_id,
                suspending_begin_cycles: minstant::now(),
                pending_event,
            }),
        }
    }

    pub fn trace_enable<E: Into<u32>>(&mut self, event: E) -> Option<LocalTraceGuard> {
        if let Some(inner) = &mut self.inner {
            let settle_event = event.into();
            let pending_event = inner.pending_event.unwrap_or(settle_event);

            let now_cycles = minstant::now();
            if let Some((local_guard, self_id)) = crate::trace_local::LocalTraceGuard::new(
                inner.collector.clone(),
                now_cycles,
                crate::LeadingSpan {
                    // At this restoring time, fill this leading span the previously reserved suspending state,
                    // related id, begin cycles and ...
                    state: inner.next_suspending_state,
                    related_id: inner.next_related_id,
                    begin_cycles: inner.suspending_begin_cycles,
                    // ... other fields calculating via them.
                    elapsed_cycles: now_cycles.saturating_sub(inner.suspending_begin_cycles),
                    event: pending_event,
                },
                settle_event,
            ) {
                // Reserve these for the next suspending process
                inner.next_suspending_state = crate::State::Scheduling;
                inner.next_related_id = self_id;

                // Obviously, the begin cycles of the next suspending is impossible to predict, and it should
                // be recorded when `local_guard` is dropping. Here `LocalTraceGuard` is for this purpose.
                // See `impl Drop for LocalTraceGuard`.
                Some(LocalTraceGuard {
                    _local: local_guard,
                    handle: inner,
                })
            } else {
                None
            }
        } else {
            None
        }
    }

    pub(crate) fn new_root(
        collector: std::sync::Arc<crate::collector::CollectorInner>,
        now_cycles: u64,
        pending_event: Option<u32>,
    ) -> Self {
        Self {
            inner: Some(AsyncTraceInner {
                collector,
                next_suspending_state: crate::State::Root,
                next_related_id: 0,
                suspending_begin_cycles: now_cycles,
                pending_event,
            }),
        }
    }
}
