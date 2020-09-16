// Copyright 2020 TiKV Project Authors. Licensed under Apache-2.0.

#[must_use]
#[inline]
pub fn trace_enable<T: Into<u32>>(
    event: T,
) -> (crate::trace_local::LocalGuard, crate::collector::Collector) {
    let event = event.into();
    trace_enable_fine(event, event, event)
}

#[must_use]
#[inline]
pub fn trace_enable_fine<E0: Into<u32>, E1: Into<u32>, E2: Into<u32>>(
    event: E0,
    pending_event: E1,
    settle_event: E2,
) -> (crate::trace_local::LocalGuard, crate::collector::Collector) {
    let now_cycles = minstant::now();
    let mut collector = crate::collector::Collector::new(crate::time::real_time_ns());

    let handle = crate::trace_async::TraceHandle::new_root(
        collector.inner.clone(),
        now_cycles,
        event.into(),
        pending_event.into(),
    );

    let guard = handle
        .trace_enable(settle_event.into())
        .unwrap() // will not panic
        .take_local_guard();
    collector.trace_handle = Some(handle);

    (guard, collector)
}

#[must_use]
#[inline]
pub fn trace_may_enable<T: Into<u32>>(
    enable: bool,
    event: T,
) -> (
    Option<crate::trace_local::LocalGuard>,
    Option<crate::collector::Collector>,
) {
    let event = event.into();
    trace_may_enable_fine(enable, event, event, event)
}

#[must_use]
#[inline]
pub fn trace_may_enable_fine<E0: Into<u32>, E1: Into<u32>, E2: Into<u32>>(
    enable: bool,
    event: E0,
    pending_event: E1,
    settle_event: E2,
) -> (
    Option<crate::trace_local::LocalGuard>,
    Option<crate::collector::Collector>,
) {
    if enable {
        let (guard, collector) = trace_enable_fine(event, pending_event, settle_event);
        (Some(guard), Some(collector))
    } else {
        (None, None)
    }
}

#[must_use]
#[inline]
pub fn new_span<T: Into<u32>>(event: T) -> Option<crate::trace_local::SpanGuard> {
    crate::trace_local::SpanGuard::new(event.into())
}

/// Bind the current tracing context to another executing context (e.g. a closure).
///
/// ```
/// # use minitrace::trace_binder;
/// # use std::thread;
/// #
/// let handle = trace_binder(EVENT0);
/// thread::spawn(move || {
///     let _g = handle.trace_enable(EVENT1);
/// });
/// ```
#[must_use]
#[inline]
pub fn trace_binder<E: Into<u32>>(event: E) -> crate::trace_async::TraceHandle {
    let event = event.into();
    crate::trace_async::TraceHandle::new(event, event)
}

#[must_use]
#[inline]
pub fn trace_binder_fine<E1: Into<u32>, E2: Into<u32>>(
    event: E1,
    pending_event: E2,
) -> crate::trace_async::TraceHandle {
    crate::trace_async::TraceHandle::new(event.into(), pending_event.into())
}

/// The property is in bytes format, so it is not limited to be a key-value pair but
/// anything intended. However, the downside of flexibility is that manual encoding
/// and manual decoding need to consider.
#[inline]
pub fn property<B: AsRef<[u8]>>(p: B) {
    crate::trace_local::append_property(|| p);
}

/// `property` of closure version
#[inline]
pub fn property_closure<F, B>(f: F)
where
    B: AsRef<[u8]>,
    F: FnOnce() -> B,
{
    crate::trace_local::append_property(f);
}
