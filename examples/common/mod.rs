// Copyright 2020 TiKV Project Authors. Licensed under Apache-2.0.

use std::collections::HashMap;

use daggy::{Dag, NodeIndex, Walker};
use minitrace::{Span, SpanId};

const BAR_LEN: usize = 70;

fn build_tree(spans: &[Span]) -> (Dag<Span, ()>, NodeIndex, u64, u64) {
    let mut dag = Dag::new();
    let mut id_map: HashMap<SpanId, NodeIndex> = HashMap::new();
    let mut root: Option<NodeIndex> = None;
    let mut min_begin = u64::max_value();
    let mut max_end = 0;

    for span in spans {
        let span_id = span.id;
        let node_id = dag.add_node(*span);
        id_map.insert(span_id, node_id);
        min_begin = min_begin.min(span.begin_cycles);
        max_end = max_end.max(span.begin_cycles + span.elapsed_cycles);
    }

    for span in spans {
        if span.parent_id != 0 {
            dag.add_edge(id_map[&span.parent_id], id_map[&span.id], ())
                .unwrap();
        } else {
            root = Some(id_map[&span.id]);
        }
    }

    (dag, root.expect("root doesn't exist"), min_begin, max_end)
}

pub fn draw_stdout(spans: &[Span], cycles_per_second: u64) {
    let (tree, root, min_begin, max_end) = build_tree(spans);
    let factor = BAR_LEN as f64 / (max_end - min_begin) as f64;

    let mut stack = vec![root];
    while !stack.is_empty() {
        let cur_index = stack.pop().unwrap();
        let span = tree.node_weight(cur_index).unwrap();
        let mut children: Vec<NodeIndex> = tree
            .children(cur_index)
            .iter(&tree)
            .map(|(_, node_idx)| node_idx)
            .collect();
        children.sort_by(|a, b| {
            tree.node_weight(*b)
                .unwrap()
                .begin_cycles
                .cmp(&tree.node_weight(*a).unwrap().begin_cycles)
        });
        stack.append(&mut children);

        // draw leading space
        let leading_space_len = ((span.begin_cycles - min_begin) as f64 * factor) as usize;
        print!("{: <1$}", "", leading_space_len);

        // draw bar
        let bar_len = (span.elapsed_cycles as f64 * factor) as usize;
        print!("{:=<1$}", "", bar_len);

        // draw tailing space
        let tailing_space_len = BAR_LEN - bar_len - leading_space_len + 1;
        print!("{: <1$}", "", tailing_space_len);

        // draw time
        println!(
            "{:6.2} ms",
            span.elapsed_cycles as f64 * 1_000.0 / cycles_per_second as f64
        );
    }
}
