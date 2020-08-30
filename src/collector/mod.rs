// Copyright 2020 TiKV Project Authors. Licensed under Apache-2.0.

mod sync_impl;

use crate::trace::SpanSet;
pub use sync_impl::{Finisher, SyncCollector, TraceResult};

pub trait Collector: Send {
    fn collect_span_set(&mut self, span_set: SpanSet);
    fn is_closed(&mut self) -> bool;
    fn clone_into_box(&mut self) -> Box<dyn Collector>;
}
