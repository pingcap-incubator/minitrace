// Copyright 2020 TiKV Project Authors. Licensed under Apache-2.0.

pub mod cycle;
pub mod span_id;

pub(crate) mod span_queue;

use crate::span::cycle::{Anchor, Cycle, DefaultClock};
use crate::span::span_id::SpanId;

#[derive(Clone, Debug, Default)]
pub struct Span {
    pub id: u32,
    pub parent_id: u32,
    pub begin_unix_time_ns: u64,
    pub duration_ns: u64,
    pub event: &'static str,
    pub properties: Vec<(&'static str, String)>,
}

#[derive(Clone, Debug)]
pub struct RawSpan {
    pub id: SpanId,
    pub parent_id: SpanId,
    pub begin_cycle: Cycle,
    pub event: &'static str,
    pub properties: Vec<(&'static str, String)>,

    // Will write this field at post processing
    pub end_cycle: Cycle,
}

impl RawSpan {
    #[inline]
    pub(crate) fn begin_with(
        id: SpanId,
        parent_id: SpanId,
        begin_cycles: Cycle,
        event: &'static str,
    ) -> Self {
        RawSpan {
            id,
            parent_id,
            begin_cycle: begin_cycles,
            event,
            properties: vec![],
            end_cycle: Cycle::default(),
        }
    }

    #[inline]
    pub(crate) fn end_with(&mut self, end_cycle: Cycle) {
        self.end_cycle = end_cycle;
    }

    #[inline]
    pub fn build_span(&self, anchor: Anchor) -> Span {
        let begin_unix_time_ns = DefaultClock::cycle_to_unix_time_ns(self.begin_cycle, anchor);
        let end_unix_time_ns = DefaultClock::cycle_to_unix_time_ns(self.end_cycle, anchor);
        Span {
            id: self.id.0,
            parent_id: self.parent_id.0,
            begin_unix_time_ns,
            duration_ns: end_unix_time_ns - begin_unix_time_ns,
            event: self.event,
            properties: self.properties.clone(),
        }
    }
}
