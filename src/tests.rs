// Copyright 2020 TiKV Project Authors. Licensed under Apache-2.0.

use crate::prelude::*;

// An auxiliary function for checking relations of spans.
// Note that the events of each spans cannot be the same.
//
// Return: [(event, Option<parent_event>)], sorted by event
fn rebuild_relation_by_event(spans: Vec<crate::SpanSet>) -> Vec<(u32, Option<u32>)> {
    let spans = spans
        .into_iter()
        .map(|s| {
            let spans = &s.spans;
            let first = spans.first().unwrap();
            if first.state != crate::State::Root {
                assert!(
                    first.state == crate::State::Scheduling
                        || first.state == crate::State::Spawning
                );
                let second = &spans[1];
                assert_eq!(second.state, crate::State::Settle);
                assert_eq!(first.event, second.event);
            }
            s.spans.into_iter()
        })
        .flatten()
        .collect::<Vec<_>>();
    let infos: std::collections::HashMap<u64, (Option<u64>, u32)> = spans
        .into_iter()
        .map(|s| {
            (
                s.id,
                (
                    match s.state {
                        crate::State::Root => None,
                        _ => Some(s.related_id),
                    },
                    s.event,
                ),
            )
        })
        .collect();

    let mut res = Vec::with_capacity(infos.len());

    for (_id, (parent_id, event)) in infos.iter() {
        if let Some(p) = parent_id {
            res.push((*event, Some(infos[&p].1)));
        } else {
            res.push((*event, None));
        }
    }

    res.sort();
    res
}

fn check_trace_local<F>(f: F)
where
    F: Fn(&crate::trace_local::TraceLocal) -> bool,
{
    crate::trace_local::TRACE_LOCAL.with(|trace| {
        let tl = unsafe { &*trace.get() };
        assert!(f(tl));
    });
}

fn check_clear() {
    check_trace_local(|tl| {
        tl.spans.is_empty()
            && tl.enter_stack.is_empty()
            && tl.cur_collector.is_none()
            && tl.property_ids.is_empty()
            && tl.property_lens.is_empty()
            && tl.property_payload.is_empty()
    });
}

#[test]
fn trace_basic() {
    let (root, collector) = crate::trace_enable(0u32);
    {
        let _guard = root;
        {
            let _guard = crate::new_span(1u32);
        }
    }

    let trace_details = collector.collect();
    let spans = rebuild_relation_by_event(trace_details.span_sets);

    assert_eq!(spans.len(), 2);
    assert_eq!(&spans, &[(0, None), (1, Some(0))]);
    check_clear();
}

#[test]
fn trace_not_enable() {
    {
        let _guard = crate::new_span(1u32);
    }

    check_clear();
}

#[test]
fn trace_async_basic() {
    let (root, collector) = crate::trace_enable(0u32);

    let wg = crossbeam::sync::WaitGroup::new();
    let mut join_handles = vec![];
    {
        let _guard = root;

        async fn dummy() {};

        for i in 1..=5u32 {
            let dummy = dummy().trace_task(i);
            let wg = wg.clone();

            join_handles.push(std::thread::spawn(move || {
                futures_03::executor::block_on(dummy);
                drop(wg);

                check_clear();
            }));
        }

        for i in 6..=10u32 {
            let handle = crate::trace_crossthread();
            let wg = wg.clone();

            join_handles.push(std::thread::spawn(move || {
                let mut handle = handle;
                let guard = handle.trace_enable(i);
                drop(guard);
                drop(wg);

                check_clear();
            }));
        }
    }

    wg.wait();
    let trace_details = collector.collect();
    let spans = rebuild_relation_by_event(trace_details.span_sets);

    assert_eq!(spans.len(), 21);
    assert_eq!(
        &spans,
        &[
            (0, None),
            (1, Some(0)),
            (1, Some(1)),
            (2, Some(0)),
            (2, Some(2)),
            (3, Some(0)),
            (3, Some(3)),
            (4, Some(0)),
            (4, Some(4)),
            (5, Some(0)),
            (5, Some(5)),
            (6, Some(0)),
            (6, Some(6)),
            (7, Some(0)),
            (7, Some(7)),
            (8, Some(0)),
            (8, Some(8)),
            (9, Some(0)),
            (9, Some(9)),
            (10, Some(0)),
            (10, Some(10)),
        ]
    );

    check_clear();
    join_handles.into_iter().for_each(|jh| jh.join().unwrap());
}

#[test]
fn trace_wide_function() {
    let (root, collector) = crate::trace_enable(0u32);

    {
        let _guard = root;
        for i in 1..=10u32 {
            let _guard = crate::new_span(i);
        }
    }

    let trace_details = collector.collect();
    let spans = rebuild_relation_by_event(trace_details.span_sets);

    assert_eq!(spans.len(), 11);
    assert_eq!(
        &spans,
        &[
            (0, None),
            (1, Some(0)),
            (2, Some(0)),
            (3, Some(0)),
            (4, Some(0)),
            (5, Some(0)),
            (6, Some(0)),
            (7, Some(0)),
            (8, Some(0)),
            (9, Some(0)),
            (10, Some(0))
        ]
    );
    check_clear();
}

#[test]
fn trace_deep_function() {
    fn sync_spanned_rec_event_step_to_1(step: u32) {
        let _guard = crate::new_span(step);

        if step > 1 {
            sync_spanned_rec_event_step_to_1(step - 1);
        }
    }

    let (root, collector) = crate::trace_enable(0u32);

    {
        let _guard = root;
        sync_spanned_rec_event_step_to_1(10);
    }

    let trace_details = collector.collect();
    let spans = rebuild_relation_by_event(trace_details.span_sets);

    assert_eq!(spans.len(), 11);
    assert_eq!(
        &spans,
        &[
            (0, None),
            (1, Some(2)),
            (2, Some(3)),
            (3, Some(4)),
            (4, Some(5)),
            (5, Some(6)),
            (6, Some(7)),
            (7, Some(8)),
            (8, Some(9)),
            (9, Some(10)),
            (10, Some(0))
        ]
    );
    check_clear();
}

#[test]
fn trace_collect_ahead() {
    let (root, collector) = crate::trace_enable(0u32);

    {
        let _guard = crate::new_span(1u32);
    }

    let wg = crossbeam::sync::WaitGroup::new();
    let wg1 = wg.clone();
    let handle = crate::trace_crossthread();
    let jh = std::thread::spawn(move || {
        let mut handle = handle;
        let guard = handle.trace_enable(2u32);

        wg1.wait();
        drop(guard);

        check_clear();
    });

    drop(root);
    let trace_details = collector.collect();
    drop(wg);

    let spans = rebuild_relation_by_event(trace_details.span_sets);
    assert_eq!(spans.len(), 2);
    assert_eq!(&spans, &[(0, None), (1, Some(0)),]);
    check_clear();

    jh.join().unwrap();
}

#[test]
fn test_property_sync() {
    let (root, collector) = crate::trace_enable(0u32);
    crate::property(b"123");

    let g1 = crate::new_span(1u32);
    let g2 = crate::new_span(2u32);
    crate::property(b"abc");
    crate::property(b"");

    let g3 = crate::new_span(2u32);
    crate::property(b"edf");

    drop(g3);
    drop(g2);
    drop(g1);
    drop(root);

    let trace_details = collector.collect();
    assert_eq!(trace_details.span_sets.len(), 1);

    let span_set = trace_details.span_sets[0].clone();
    assert_eq!(span_set.spans.len(), 4);
    assert_eq!(span_set.properties.span_ids.len(), 4);
    assert_eq!(span_set.properties.span_lens.len(), 4);
    assert_eq!(span_set.properties.payload.len(), 9);
    assert_eq!(span_set.properties.payload, b"123abcedf");

    for (x, y) in [
        span_set.spans[0].id,
        span_set.spans[2].id,
        span_set.spans[2].id,
        span_set.spans[3].id,
    ]
    .iter()
    .zip(span_set.properties.span_ids)
    {
        assert_eq!(*x, y);
    }
    for (x, y) in [3, 3, 0, 3].iter().zip(span_set.properties.span_lens) {
        assert_eq!(*x, y);
    }

    check_clear();
}

#[test]
fn test_property_async() {
    let (root, collector) = crate::trace_enable(0u32);

    let wg = crossbeam::sync::WaitGroup::new();
    let mut join_handles = vec![];

    {
        let _guard = root;
        crate::property(&0u32.to_be_bytes());

        for i in 1..=5u32 {
            let handle = crate::trace_crossthread();
            let wg = wg.clone();

            join_handles.push(std::thread::spawn(move || {
                let mut handle = handle;
                let guard = handle.trace_enable(i);
                crate::property(&i.to_be_bytes());
                drop(guard);
                drop(wg);

                check_clear();
            }));
        }
    }

    wg.wait();

    let trace_details = collector.collect();
    for span_set in trace_details.span_sets {
        let (id, event) = match span_set.spans.len() {
            2 => (span_set.spans[1].id, span_set.spans[1].event),
            1 => (span_set.spans[0].id, span_set.spans[0].event),
            _ => panic!("unexpected len: {}", span_set.spans.len()),
        };

        assert_eq!(span_set.properties.span_ids.len(), 1);
        assert_eq!(span_set.properties.span_ids[0], id);
        assert_eq!(span_set.properties.span_lens.len(), 1);
        assert_eq!(span_set.properties.span_lens[0], 4);
        assert_eq!(span_set.properties.payload, event.to_be_bytes());
    }

    check_clear();
    join_handles.into_iter().for_each(|jh| jh.join().unwrap());
}
