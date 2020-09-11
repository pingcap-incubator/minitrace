// Copyright 2020 TiKV Project Authors. Licensed under Apache-2.0.

use crate::prelude::*;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Debug)]
struct LeadingNode {
    span: crate::Span,
    child: Option<Rc<RefCell<NormalNode>>>,
    next: Option<Rc<RefCell<LeadingNode>>>,
}

#[derive(Debug)]
struct NormalNode {
    span: crate::Span,
    normal_children: Vec<Rc<RefCell<NormalNode>>>,
    leading_children: Vec<Rc<RefCell<LeadingNode>>>,
}

#[derive(Debug)]
enum Node {
    LeadingNode(Rc<RefCell<LeadingNode>>),
    NormalNode(Rc<RefCell<NormalNode>>),
}

fn build_tree(trace_details: &crate::TraceDetails) -> Rc<RefCell<LeadingNode>> {
    let mut spans = trace_details.spans.clone();
    spans.sort_by(|a, b| a.begin_cycles.cmp(&b.begin_cycles));
    let mut id_to_node: HashMap<u32, Node> = HashMap::new();

    let mut root = None;
    let mut idle = vec![];
    for span in spans {
        let mut process = idle.clone();
        process.push(span);
        idle.clear();
        for span in process {
            match span.state {
                crate::State::Root => {
                    let leading_node = Rc::new(RefCell::new(LeadingNode {
                        span,
                        child: None,
                        next: None,
                    }));
                    id_to_node.insert(span.id, Node::LeadingNode(leading_node.clone()));
                    root = Some(leading_node);
                }
                crate::State::Local => match id_to_node.get(&span.related_id) {
                    Some(Node::NormalNode(parent)) => {
                        let normal_node = Rc::new(RefCell::new(NormalNode {
                            span,
                            normal_children: vec![],
                            leading_children: vec![],
                        }));
                        parent
                            .borrow_mut()
                            .normal_children
                            .push(normal_node.clone());
                        id_to_node.insert(span.id, Node::NormalNode(normal_node));
                    }
                    Some(_) => unreachable!(),
                    None => {
                        idle.push(span);
                    }
                },
                crate::State::Spawning => match id_to_node.get(&span.related_id) {
                    Some(Node::NormalNode(parent)) => {
                        let leading_node = Rc::new(RefCell::new(LeadingNode {
                            span,
                            child: None,
                            next: None,
                        }));
                        parent
                            .borrow_mut()
                            .leading_children
                            .push(leading_node.clone());
                        id_to_node.insert(span.id, Node::LeadingNode(leading_node));
                    }
                    Some(_) => unreachable!(),
                    None => idle.push(span),
                },
                crate::State::Scheduling => match id_to_node.get(&span.related_id) {
                    Some(Node::LeadingNode(prev)) => {
                        let leading_node = Rc::new(RefCell::new(LeadingNode {
                            span,
                            child: None,
                            next: None,
                        }));
                        prev.borrow_mut().next = Some(leading_node.clone());
                        id_to_node.insert(span.id, Node::LeadingNode(leading_node));
                    }
                    Some(_) => unreachable!(),
                    None => idle.push(span),
                },
                crate::State::Settle => match id_to_node.get(&span.related_id) {
                    Some(Node::LeadingNode(prev)) => {
                        let normal_node = Rc::new(RefCell::new(NormalNode {
                            span,
                            normal_children: vec![],
                            leading_children: vec![],
                        }));
                        prev.borrow_mut().child = Some(normal_node.clone());
                        id_to_node.insert(span.id, Node::NormalNode(normal_node));
                    }
                    Some(_) => unreachable!(),
                    None => idle.push(span),
                },
            }
        }
    }
    assert!(idle.is_empty());

    root.expect("root span isn't existing")
}

fn compare_relation(real_tree: &Rc<RefCell<LeadingNode>>, shape: &Rc<RefCell<LeadingNode>>) {
    compare_leading(real_tree, shape);
}

fn compare_leading(real_tree: &Rc<RefCell<LeadingNode>>, approx_tree: &Rc<RefCell<LeadingNode>>) {
    assert_eq!(
        real_tree.borrow().span.event,
        approx_tree.borrow().span.event
    );

    if real_tree.borrow().next.is_some() || approx_tree.borrow().next.is_some() {
        compare_leading(
            real_tree.borrow().next.as_ref().unwrap(),
            approx_tree.borrow().next.as_ref().unwrap(),
        );
    }

    compare_normal(
        &real_tree.borrow().child.as_ref().unwrap(),
        &approx_tree.borrow().child.as_ref().unwrap(),
    );
}

fn compare_normal(real_tree: &Rc<RefCell<NormalNode>>, approx_tree: &Rc<RefCell<NormalNode>>) {
    assert_eq!(
        real_tree.borrow().span.event,
        approx_tree.borrow().span.event
    );
    {
        assert_eq!(
            real_tree.borrow().normal_children.len(),
            approx_tree.borrow().normal_children.len()
        );
        for (real, approc) in real_tree
            .borrow()
            .normal_children
            .iter()
            .zip(approx_tree.borrow().normal_children.iter())
        {
            compare_normal(real, approc);
        }
    }
    {
        real_tree
            .borrow_mut()
            .leading_children
            .sort_by(|a, b| a.borrow().span.event.cmp(&b.borrow().span.event));

        approx_tree
            .borrow_mut()
            .leading_children
            .sort_by(|a, b| a.borrow().span.event.cmp(&b.borrow().span.event));
    }

    assert_eq!(
        real_tree.borrow().leading_children.len(),
        approx_tree.borrow().leading_children.len()
    );
    for (real, approc) in real_tree
        .borrow()
        .leading_children
        .iter()
        .zip(approx_tree.borrow().leading_children.iter())
    {
        compare_leading(real, approc);
    }
}

macro_rules! leading {
    ($event:expr, $child:expr, $next:expr) => {
        Rc::new(RefCell::new(LeadingNode {
            span: crate::Span {
                id: 0,
                state: crate::State::Root,
                related_id: 0,
                begin_cycles: 0,
                elapsed_cycles: 0,
                event: $event,
            },
            child: Some($child),
            next: $next,
        }))
    };
    ($event:expr, child: $child:expr, next: $next:expr) => {
        leading!($event, $child, Some($next))
    };
    ($event:expr, child: $child:expr) => {
        leading!($event, $child, None)
    };
}

macro_rules! normal {
    ($event:expr, normals: [$($normal:expr),*], leadings: [$($leading:expr),*]) => {
        Rc::new(RefCell::new(NormalNode {
            span: crate::Span {
                id: 0,
                state: crate::State::Root,
                related_id: 0,
                begin_cycles: 0,
                elapsed_cycles: 0,
                event: $event,
            },
            normal_children: vec![$($normal,)*],
            leading_children: vec![$($leading,)*],
        }))
    };
    ($event:expr) => {
        normal!($event, normals: [], leadings: [])
    };
    ($event:expr, normals: [$($normal:expr),*]) => {
        normal!($event, normals: [$($normal),*], leadings: [])
    };
    ($event:expr, leadings: [$($leading:expr),*]) => {
        normal!($event, normals: [], leadings: [$($leading),*])
    };
}

fn check_time_included(tree: &Rc<RefCell<LeadingNode>>) {
    fn check_normal(tree: &Rc<RefCell<NormalNode>>) {
        let node = &*tree.borrow();
        let begin_cycles = node.span.begin_cycles;
        let end_cycles = node.span.begin_cycles + node.span.elapsed_cycles;

        let mut prev_end_cycles = 0;
        for normal in &node.normal_children {
            let span = &normal.borrow().span;
            assert!(prev_end_cycles <= span.begin_cycles);
            assert!(begin_cycles <= span.begin_cycles);
            assert!(span.begin_cycles + span.elapsed_cycles <= end_cycles);
            prev_end_cycles = span.begin_cycles + span.elapsed_cycles;
            check_normal(normal);
        }

        for leading in &node.leading_children {
            let span = &leading.borrow().span;
            assert!(begin_cycles <= span.begin_cycles);
            assert!(span.begin_cycles <= end_cycles);
            check_time_included(leading);
        }
    }
    let leading = &*tree.borrow();
    assert_eq!(
        leading.span.begin_cycles + leading.span.elapsed_cycles,
        leading.child.as_ref().unwrap().borrow().span.begin_cycles
    );
    check_normal(leading.child.as_ref().unwrap());

    let prev_span = &leading.child.as_ref().unwrap().borrow().span;
    let prev_end_cycles = prev_span.begin_cycles + prev_span.elapsed_cycles;
    if let Some(next) = &leading.next {
        assert!(prev_end_cycles <= next.borrow().span.begin_cycles);
        check_time_included(next);
    }
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
    let collector = {
        let (_guard, collector) = crate::trace_enable(0u32);

        {
            let _guard = crate::new_span(1u32);
        }

        collector
    };

    let trace_details = collector.collect();

    let real_tree = build_tree(&trace_details);
    let shape = leading!(0, child: normal!(0, normals: [normal!(1)]));
    compare_relation(&real_tree, &shape);
    check_time_included(&real_tree);
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
    let wg = crossbeam::sync::WaitGroup::new();
    let mut join_handles = vec![];
    let collector = {
        let (_guard, collector) = crate::trace_enable(0u32);

        for i in 1..=5u32 {
            let dummy = async {}.trace_task(i);
            let wg = wg.clone();

            join_handles.push(std::thread::spawn(move || {
                futures_03::executor::block_on(dummy);
                drop(wg);

                check_clear();
            }));
        }

        for i in 6..=10u32 {
            let mut handle = crate::trace_binder(i);
            let wg = wg.clone();

            join_handles.push(std::thread::spawn(move || {
                let guard = handle.trace_enable(i);
                drop(guard);
                drop(wg);

                check_clear();
            }));
        }

        collector
    };

    wg.wait();
    let trace_details = collector.collect();

    let real_tree = build_tree(&trace_details);
    let shape = leading!(
        0,
        child:
            normal!(0,
                leadings:
                    [
                        leading!(1, child: normal!(1)),
                        leading!(2, child: normal!(2)),
                        leading!(3, child: normal!(3)),
                        leading!(4, child: normal!(4)),
                        leading!(5, child: normal!(5)),
                        leading!(6, child: normal!(6)),
                        leading!(7, child: normal!(7)),
                        leading!(8, child: normal!(8)),
                        leading!(9, child: normal!(9)),
                        leading!(10, child: normal!(10))
                    ]
            )
    );
    compare_relation(&real_tree, &shape);
    check_time_included(&real_tree);
    check_clear();
    join_handles.into_iter().for_each(|jh| jh.join().unwrap());
}

#[test]
fn trace_wide_function() {
    let collector = {
        let (_guard, collector) = crate::trace_enable(0u32);

        for i in 1..=10u32 {
            let _guard = crate::new_span(i);
        }

        collector
    };

    let trace_details = collector.collect();

    let real_tree = build_tree(&trace_details);
    let shape = leading!(
        0,
        child:
            normal!(0,
                normals:
                    [
                        normal!(1),
                        normal!(2),
                        normal!(3),
                        normal!(4),
                        normal!(5),
                        normal!(6),
                        normal!(7),
                        normal!(8),
                        normal!(9),
                        normal!(10)
                    ]
            )
    );
    compare_relation(&real_tree, &shape);
    check_time_included(&real_tree);
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

    let collector = {
        let (_guard, collector) = crate::trace_enable(0u32);

        sync_spanned_rec_event_step_to_1(10);

        collector
    };

    let trace_details = collector.collect();

    let real_tree = build_tree(&trace_details);
    let shape = leading!(
        0,
        child:
            normal!(
                0,
                normals:
                    [normal!(
                        10,
                        normals:
                            [normal!(
                                9,
                                normals:
                                    [normal!(
                                        8,
                                        normals:
                                            [normal!(
                                                7,
                                                normals:
                                                    [normal!(
                                                        6,
                                                        normals:
                                                            [normal!(
                                                                5,
                                                                normals:
                                                                    [normal!(
                                                                        4,
                                                                        normals:
                                                                            [normal!(
                                                                                3,
                                                                                normals:
                                                                                    [normal!(
                                                                                        2,
                                                                                        normals:
                                                                                            [normal!(
                                                                                                1
                                                                                            )]
                                                                                    )]
                                                                            )]
                                                                    )]
                                                            )]
                                                    )]
                                            )]
                                    )]
                            )]
                    )]
            )
    );
    compare_relation(&real_tree, &shape);
    check_time_included(&real_tree);
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
    let mut handle = crate::trace_binder(2u32);
    let jh = std::thread::spawn(move || {
        let guard = handle.trace_enable(2u32);

        wg1.wait();
        drop(guard);

        check_clear();
    });

    drop(root);
    let trace_details = collector.collect();
    drop(wg);

    let real_tree = build_tree(&trace_details);
    let shape = leading!(0, child: normal!(0, normals: [normal!(1)]));
    compare_relation(&real_tree, &shape);
    check_time_included(&real_tree);

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

    let g3 = crate::new_span(3u32);
    crate::property(b"edf");

    drop(g3);
    drop(g2);
    drop(g1);
    drop(root);

    let trace_details = collector.collect();

    let real_tree = build_tree(&trace_details);
    let shape = leading!(
        0,
        child:
            normal!(
                0,
                normals: [normal!(1, normals: [normal!(2, normals: [normal!(3)])])]
            )
    );
    compare_relation(&real_tree, &shape);
    check_time_included(&real_tree);
    check_clear();

    assert_eq!(trace_details.properties.span_ids.len(), 4);
    assert_eq!(trace_details.properties.property_lens.len(), 4);
    assert_eq!(trace_details.properties.payload.len(), 9);
    assert_eq!(trace_details.properties.payload, b"123abcedf");

    for (x, y) in [
        trace_details.spans[1].id,
        trace_details.spans[3].id,
        trace_details.spans[3].id,
        trace_details.spans[4].id,
    ]
    .iter()
    .zip(trace_details.properties.span_ids)
    {
        assert_eq!(*x, y);
    }
    for (x, y) in [3, 3, 0, 3]
        .iter()
        .zip(trace_details.properties.property_lens)
    {
        assert_eq!(*x, y);
    }
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
            let mut handle = crate::trace_binder(i);
            let wg = wg.clone();

            join_handles.push(std::thread::spawn(move || {
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
    let real_tree = build_tree(&trace_details);
    let shape = leading!(
        0,
        child:
            normal!(0, leadings: [
                leading!(1, child: normal!(1)),
                leading!(2, child: normal!(2)),
                leading!(3, child: normal!(3)),
                leading!(4, child: normal!(4)),
                leading!(5, child: normal!(5))
            ])
    );
    compare_relation(&real_tree, &shape);
    check_time_included(&real_tree);
    check_clear();
    join_handles.into_iter().for_each(|jh| jh.join().unwrap());

    assert_eq!(trace_details.properties.span_ids.len(), 6);
    assert_eq!(trace_details.properties.property_lens.len(), 6);
    let u32_size = std::mem::size_of::<u32>();
    assert_eq!(trace_details.properties.payload.len(), 6 * u32_size);

    for property_len in trace_details.properties.property_lens {
        assert_eq!(property_len, std::mem::size_of::<u32>() as u64);
    }

    for i in 0..6 {
        let id = trace_details.properties.span_ids[i];
        let mut event = 0;
        for span in &trace_details.spans {
            if span.id == id {
                event = span.event;
            }
        }
        assert_eq!(
            trace_details.properties.payload[i * u32_size..(i + 1) * u32_size],
            event.to_be_bytes()
        );
    }
}
