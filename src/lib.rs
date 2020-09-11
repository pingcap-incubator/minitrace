// Copyright 2020 TiKV Project Authors. Licensed under Apache-2.0.

#![feature(negative_impls)]
#![feature(shrink_to)]

pub(crate) mod collector;
pub mod future;
pub mod prelude;
pub(crate) mod time;
pub(crate) mod trace;
pub(crate) mod trace_async;
pub(crate) mod trace_local;

#[cfg(feature = "jaeger")]
pub mod jaeger;

pub use collector::*;
pub use trace::*;
pub use trace_async::*;
pub use trace_local::*;

// #[cfg(test)]
// mod tests;

pub use minitrace_attribute::{trace, trace_async};

#[derive(Debug, Clone)]
pub struct TraceDetails {
    /// The start time of the whole tracing process that is the time
    /// when calling `trace_enable`
    pub start_time_ns: u64,

    /// The elapsed of the whole tracing process that is the time diff
    /// from calling `trace_enable` to calling `collect`
    pub elapsed_ns: u64,

    /// For conversion of cycles -> ns
    pub cycles_per_second: u64,

    /// Span collection
    pub spans: Vec<Span>,

    /// Properties
    pub properties: Properties,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct Span {
    pub id: u32,
    pub state: State,
    pub related_id: u32,
    pub begin_cycles: u64,
    pub elapsed_cycles: u64,
    pub event: u32,
}

/// `State` represents the attributes of a span and how the span relates to
/// another span with `related_id`.
///
/// ## Root
/// ```text
/// ------------------------------- TIME LINE -------------------------------------->
///
/// | <- enable trace
/// +------------------------------------+-------------------------------+
/// | state: Root, related_id: 0, id: 88 | state: Settle, related_id: 88 |
/// +------------------------------------+-------------------------------+
/// ```
///
/// ## Local
/// ```text
/// ------------------------------- TIME LINE -------------------------------------->
///
/// +--------------------------------------+
/// |          A span with id 42           |
/// +--------------------------------------+
///       | <- new_span()
///       +------------------------------+
///       | state: Local, related_id: 42 |
///       +------------------------------+
/// ```
///
/// ## Spawning & Settle
/// ```text
/// ------------------------------- TIME LINE -------------------------------------->
///
/// +--------------------------------------+
/// |          A span with id 42           |
/// +--------------------------------------+
///       | let handle = trace_binder();
///       | <- thread::spawn()
///       +-----------------------------------------+-------------------------------+
///       | state: Spawning, related_id: 42, id: 77 | state: Settle, related_id: 77 |
///       +-----------------------------------------+-------------------------------+
///                                                 | <- handle.trace_enable()
/// ```
///
/// ## Scheduling & Settle
/// ```text
/// ------------------------------- TIME LINE -------------------------------------->
///
/// +--------------------------------------+
/// |          A span with id 42           |
/// +--------------------------------------+
///   | <- runtime::spawn(future)
///   +-----------------------------------------+-------------------------------+
///   | state: Spawning, related_id: 42, id: 77 | state: Settle, related_id: 77 |
///   +-----------------------------------------+-------------------------------+
///                                             | <- future.poll()              | <- poll() return                          | <- future.poll()
///                                                                             +-------------------------------------------+-------------------------------+
///                                                                             | state: Scheduling, related_id: 77, id: 23 | state: Settle, related_id: 23 |
///                                                                             +-------------------------------------------+-------------------------------+
/// ```
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum State {
    Root,
    Local,
    Spawning,
    Scheduling,
    Settle,
}

/// Properties can used to attach some information about tracing context
/// to current span, e.g. host of the request, CPU usage.
///
/// Usage:
/// ```
/// # let event_id = 1u32;
/// let _guard = minitrace::new_span(event_id);
/// minitrace::property(b"host:127.0.0.1");
/// minitrace::property(b"cpu_usage:42%");
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
    pub span_ids: Vec<u32>,
    pub property_lens: Vec<u64>,
    pub payload: Vec<u8>,
}
