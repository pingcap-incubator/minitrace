// Copyright 2020 TiKV Project Authors. Licensed under Apache-2.0.

use std::marker::PhantomData;
use std::sync::atomic::{AtomicU16, Ordering};

use crate::collector::Collector;
use crate::utils::real_time_ns;

pub type SpanId = u32;
pub type TraceId = u64;

static GLOBAL_ID_COUNTER: AtomicU16 = AtomicU16::new(1);

pub(crate) const INIT_LEN: usize = 1024;
pub(crate) const INIT_BYTES_LEN: usize = 16384;

thread_local! {
    pub static TRACE_LOCAL: std::cell::UnsafeCell<TraceLocal> = std::cell::UnsafeCell::new(TraceLocal {
        id_prefix: next_global_id_prefix(),
        id_suffix: 0,
        enter_stack: Vec::with_capacity(INIT_LEN),
        spans: Vec::with_capacity(INIT_LEN),
        properties: Properties {
            span_ids: Vec::with_capacity(INIT_LEN),
            property_lens: Vec::with_capacity(INIT_LEN),
            payload: Vec::with_capacity(INIT_BYTES_LEN),
        },

        trace_id: 0,
        start_time_ns: 0,

        // `Option` just for easily taking. If it's in a tracing context,
        // i.e. `enter_stack` is not empty, it will always be a `Some`.
        collector: None,
    });
}

fn next_global_id_prefix() -> u16 {
    GLOBAL_ID_COUNTER.fetch_add(1, Ordering::AcqRel)
}

pub fn start_trace<T: Into<u32>>(
    trace_id: TraceId,
    root_event: T,
    collector: Box<dyn Collector>,
) -> Option<ScopeGuard> {
    let trace = TRACE_LOCAL.with(|trace| trace.get());
    let tl = unsafe { &mut *trace };

    if !tl.enter_stack.is_empty() {
        return None;
    }

    // `minstant::now()` may need initialization, so we should fetch timestamps in this order.
    let begin_cycles = minstant::now();
    let start_time_ns = real_time_ns();

    let span_inner = SpanGuardInner::enter(
        Span {
            id: tl.new_span_id(),
            state: State::Normal,
            parent_id: 0,
            begin_cycles,
            elapsed_cycles: 0,
            event: root_event.into(),
        },
        tl,
    );

    tl.trace_id = trace_id;
    tl.start_time_ns = start_time_ns;
    tl.collector = Some(collector);

    Some(ScopeGuard { inner: span_inner })
}

pub fn new_span<T: Into<u32>>(event: T) -> Option<SpanGuard> {
    let trace = TRACE_LOCAL.with(|trace| trace.get());
    let tl = unsafe { &mut *trace };

    if tl.enter_stack.is_empty() {
        return None;
    }

    let parent_id = *tl.enter_stack.last().unwrap();
    let span_inner = SpanGuardInner::enter(
        Span {
            id: tl.new_span_id(),
            state: State::Normal,
            parent_id,
            begin_cycles: minstant::now(),
            elapsed_cycles: 0,
            event: event.into(),
        },
        tl,
    );

    Some(SpanGuard { inner: span_inner })
}

/// The property is in bytes format, so it is not limited to be a key-value pair but
/// anything intended. However, the downside of flexibility is that manual encoding
/// and manual decoding need to consider.
pub fn new_property<B: AsRef<[u8]>>(p: B) {
    append_property(|| p);
}

/// `property` of closure version
pub fn new_property_with<F, B>(f: F)
where
    B: AsRef<[u8]>,
    F: FnOnce() -> B,
{
    append_property(f);
}

pub struct TraceLocal {
    /// For id construction
    pub id_prefix: u16,
    pub id_suffix: u16,

    /// For parent-child relation construction. The last span, when exits, is
    /// responsible to submit the local span sets.
    pub enter_stack: Vec<SpanId>,

    pub spans: Vec<Span>,
    pub properties: Properties,

    pub trace_id: TraceId,
    pub start_time_ns: u64,
    pub collector: Option<Box<dyn Collector>>,
}

impl TraceLocal {
    #[inline]
    pub fn new_span_id(&mut self) -> SpanId {
        let id = ((self.id_prefix as u32) << 16) | self.id_suffix as u32;

        if self.id_suffix == std::u16::MAX {
            self.id_suffix = 0;
            self.id_prefix = next_global_id_prefix();
        } else {
            self.id_suffix += 1;
        }

        id
    }

    #[inline]
    pub fn take_spans_and_properties(&mut self) -> (Vec<Span>, Properties) {
        let spans = std::mem::replace(&mut self.spans, Vec::with_capacity(INIT_LEN));
        let properties = std::mem::replace(
            &mut self.properties,
            Properties {
                span_ids: Vec::with_capacity(INIT_LEN),
                property_lens: Vec::with_capacity(INIT_LEN),
                payload: Vec::with_capacity(INIT_BYTES_LEN),
            },
        );

        (spans, properties)
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum State {
    Normal,
    Pending,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct Span {
    pub id: SpanId,
    pub state: State,
    pub parent_id: SpanId,
    pub begin_cycles: u64,
    pub elapsed_cycles: u64,
    pub event: u32,
}

/// Properties can used to attach some information about tracing context
/// to current span, e.g. host of the request, CPU usage.
///
/// Usage:
/// ```
/// # let event_id = 1u32;
/// let _guard = minitrace::new_span(event_id);
/// minitrace::new_property(b"host:127.0.0.1");
/// minitrace::new_property(b"cpu_usage:42%");
/// ```
///
/// Every property will relate to a span. Logically properties are a sequence
/// of (span id, property) pairs:
/// ```text
/// span id -> property
/// 10      -> b"123"
/// 10      -> b"!@$#$%"
/// 12      -> b"abcd"
/// 14      -> b"xyz"
/// ```
///
/// and will be stored into `Properties` struct as:
/// ```text
/// span_ids: [10, 10, 12, 14]
/// property_lens: [3, 6, 4, 3]
/// payload: b"123!@$#$%abcdxyz"
/// ```
#[derive(Debug, Clone, Default)]
pub struct Properties {
    pub span_ids: Vec<SpanId>,
    pub property_lens: Vec<u64>,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, Default)]
pub struct SpanSet {
    pub trace_id: TraceId,

    pub start_time_ns: u64,

    pub cycles_per_second: u64,

    /// Span collection
    pub spans: Vec<Span>,

    /// Property collection
    pub properties: Properties,
}

#[must_use]
pub struct ScopeGuard {
    inner: SpanGuardInner,
}

impl Drop for ScopeGuard {
    #[inline]
    fn drop(&mut self) {
        let trace = TRACE_LOCAL.with(|trace| trace.get());
        let tl = unsafe { &mut *trace };

        self.inner.exit(tl);

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

#[must_use]
pub struct SpanGuard {
    pub(crate) inner: SpanGuardInner,
}

impl Drop for SpanGuard {
    #[inline]
    fn drop(&mut self) {
        let trace = TRACE_LOCAL.with(|trace| trace.get());
        let tl = unsafe { &mut *trace };

        self.inner.exit(tl);
    }
}

#[must_use]
pub struct SpanGuardInner {
    span_index: usize,
    _marker: PhantomData<*const ()>,
}

impl SpanGuardInner {
    #[inline]
    pub fn enter(span: Span, tl: &mut TraceLocal) -> Self {
        tl.enter_stack.push(span.id);
        tl.spans.push(span);

        Self {
            span_index: tl.spans.len() - 1,
            _marker: PhantomData,
        }
    }

    #[inline]
    pub fn exit(&mut self, tl: &mut TraceLocal) -> u64 {
        debug_assert_eq!(
            *tl.enter_stack.last().unwrap(),
            tl.spans[self.span_index].id
        );

        tl.enter_stack.pop();

        let now_cycle = minstant::now();
        let span = &mut tl.spans[self.span_index];
        span.elapsed_cycles = now_cycle.wrapping_sub(span.begin_cycles);

        now_cycle
    }
}

fn append_property<F, B>(f: F)
where
    B: AsRef<[u8]>,
    F: FnOnce() -> B,
{
    let trace = TRACE_LOCAL.with(|trace| trace.get());
    let tl = unsafe { &mut *trace };

    if tl.enter_stack.is_empty() {
        return;
    }

    let cur_span_id = *tl.enter_stack.last().unwrap();
    let payload = f();
    let payload = payload.as_ref();
    let payload_len = payload.len();

    tl.properties.span_ids.push(cur_span_id);
    tl.properties.property_lens.push(payload_len as u64);
    tl.properties.payload.extend_from_slice(payload);
}
