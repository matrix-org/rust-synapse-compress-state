use std::collections::BTreeMap;
use std::{fs::File, io::Write};

use super::StateGroupEntry;

type Graph = BTreeMap<i64, StateGroupEntry>;

fn output_csv(groups: &Graph, edges_output: &mut File, nodes_output: &mut File) {
    writeln!(edges_output, "Source;Target",).unwrap();

    writeln!(nodes_output, "Id;Rows;Root;Label",).unwrap();

    for (source, entry) in groups {
        if let Some(target) = entry.prev_state_group {
            writeln!(edges_output, "{};{}", source, target,).unwrap();
        }

        writeln!(
            nodes_output,
            "{};{};{};\"{}\"",
            source,
            entry.state_map.len(),
            entry.prev_state_group.is_none(),
            entry.state_map.len(),
        )
        .unwrap();
    }
}

pub fn make_graphs(before: Graph, after: Graph) {
    let mut before_edges_file = File::create("before_edges.csv").unwrap();
    let mut before_nodes_file = File::create("before_nodes.csv").unwrap();
    let mut after_edges_file = File::create("after_edges.csv").unwrap();
    let mut after_nodes_file = File::create("after_nodes.csv").unwrap();

    output_csv(&before, &mut before_edges_file, &mut before_nodes_file);
    output_csv(&after, &mut after_edges_file, &mut after_nodes_file);
}
