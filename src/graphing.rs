use std::{collections::BTreeMap, fs::File, io::Write};

use super::StateGroupEntry;

type Graph = BTreeMap<i64, StateGroupEntry>;

/// Outputs information from a state group graph into an edges file and a node file
///
/// These can be loaded into something like Gephi to visualise the graphs
///
/// # Arguments
///
/// * `groups`          - A map from state group ids to StateGroupEntries
/// * `edges_output`    - The file to output the predecessor link information to
/// * `nodes_output`    - The file to output the state group information to
fn output_csv(groups: &Graph, edges_output: &mut File, nodes_output: &mut File) {
    // The line A;B in the edges file means:
    //      That state group A has predecessor B
    writeln!(edges_output, "Source;Target",).unwrap();

    // The line A;B;C;"B" in the nodes file means:
    //      The state group id is A
    //      This state group has B rows in the state_groups_state table
    //      If C is true then A has no predecessor
    writeln!(nodes_output, "Id;Rows;Root;Label",).unwrap();

    for (source, entry) in groups {
        // If the group has a predecessor then write an edge in the edges file
        if let Some(target) = entry.prev_state_group {
            writeln!(edges_output, "{};{}", source, target,).unwrap();
        }

        // Write the state group's information to the nodes file
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

/// Outputs information from two state group graph into files
///
/// These can be loaded into something like Gephi to visualise the graphs
/// before and after the compressor is run
///
/// # Arguments
///
/// * `before`      - A map from state group ids to StateGroupEntries
///                   the information from this map goes into before_edges.csv
///                   and before_nodes.csv
/// * `after`       - A map from state group ids to StateGroupEntries
///                   the information from this map goes into after_edges.csv
///                   and after_nodes.csv
pub fn make_graphs(before: &Graph, after: &Graph) {
    // Open all the files to output to
    let mut before_edges_file = File::create("before_edges.csv").unwrap();
    let mut before_nodes_file = File::create("before_nodes.csv").unwrap();
    let mut after_edges_file = File::create("after_edges.csv").unwrap();
    let mut after_nodes_file = File::create("after_nodes.csv").unwrap();

    // Write before's information to before_edges and before_nodes
    output_csv(before, &mut before_edges_file, &mut before_nodes_file);
    // Write afters's information to after_edges and after_nodes
    output_csv(after, &mut after_edges_file, &mut after_nodes_file);
}
