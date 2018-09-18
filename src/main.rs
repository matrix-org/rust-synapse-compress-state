//! This is a tool that attempts to further compress state maps within a
//! Synapse instance's database. Specifically, it aims to reduce the number of
//! rows that a given room takes up in the `state_groups_state` table.

#[macro_use]
extern crate clap;
extern crate fallible_iterator;
extern crate indicatif;
extern crate postgres;
extern crate rayon;
extern crate rust_matrix_lib;

mod compressor;
mod database;

use compressor::Compressor;

use clap::{App, Arg};
use rayon::prelude::*;
use rust_matrix_lib::state_map::StateMap;

use std::collections::BTreeMap;
use std::fs::File;
use std::io::Write;
use std::str::FromStr;

/// An entry for a state group. Consists of an (optional) previous group and the
/// delta from that previous group (or the full state if no previous group)
#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub struct StateGroupEntry {
    prev_state_group: Option<i64>,
    state_map: StateMap<String>,
}

/// Gets the full state for a given group from the map (of deltas)
pub fn collapse_state_maps(
    map: &BTreeMap<i64, StateGroupEntry>,
    state_group: i64,
) -> StateMap<String> {
    let mut entry = &map[&state_group];
    let mut state_map = StateMap::new();

    let mut stack = vec![state_group];

    while let Some(prev_state_group) = entry.prev_state_group {
        stack.push(prev_state_group);
        if !map.contains_key(&prev_state_group) {
            panic!("Missing {}", prev_state_group);
        }
        entry = &map[&prev_state_group];
    }

    for sg in stack.iter().rev() {
        state_map.extend(
            map[&sg]
                .state_map
                .iter()
                .map(|((t, s), e)| ((t, s), e.clone())),
        );
    }

    state_map
}

/// Helper struct for parsing the `level_sizes` argument.
struct LevelSizes(Vec<usize>);

impl FromStr for LevelSizes {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut sizes = Vec::new();

        for size_str in s.split(",") {
            let size: usize = size_str
                .parse()
                .map_err(|_| "Not a comma separated list of numbers")?;
            sizes.push(size);
        }

        Ok(LevelSizes(sizes))
    }
}

fn main() {
    let matches = App::new(crate_name!())
        .version(crate_version!())
        .author(crate_authors!("\n"))
        .about(crate_description!())
        .arg(
            Arg::with_name("postgres-url")
                .short("p")
                .value_name("URL")
                .help("The url for connecting to the postgres database")
                .takes_value(true)
                .required(true),
        ).arg(
            Arg::with_name("room_id")
                .short("r")
                .value_name("ROOM_ID")
                .help("The room to process")
                .takes_value(true)
                .required(true),
        ).arg(
            Arg::with_name("output_file")
                .short("o")
                .value_name("FILE")
                .help("File to output the changes to in SQL")
                .takes_value(true),
        ).arg(
            Arg::with_name("individual_transactions")
                .short("t")
                .help("Whether to wrap each state group change in a transaction, when writing to file")
                .requires("output_file"),
        ).arg(
            Arg::with_name("level_sizes")
                .short("l")
                .value_name("LEVELS")
                .help("Sizes of each new level in the compression algorithm, as a comma separate list")
                .default_value("100,50,25")
                .takes_value(true),
        ).get_matches();

    let db_url = matches
        .value_of("postgres-url")
        .expect("db url should be required");

    let mut output_file = matches
        .value_of("output_file")
        .map(|path| File::create(path).unwrap());
    let room_id = matches
        .value_of("room_id")
        .expect("room_id should be required since no file");

    let individual_transactions = matches.is_present("individual_transactions");

    let level_sizes = value_t_or_exit!(matches, "level_sizes", LevelSizes);

    // First we need to get the current state groups
    println!("Fetching state from DB for room '{}'...", room_id);
    let state_group_map = database::get_data_from_db(db_url, room_id);

    println!("Number of state groups: {}", state_group_map.len());

    let original_summed_size = state_group_map
        .iter()
        .fold(0, |acc, (_, v)| acc + v.state_map.len());

    println!("Number of rows in current table: {}", original_summed_size);

    // Now we actually call the compression algorithm.

    let compressor = Compressor::compress(&state_group_map, &level_sizes.0);

    let new_state_group_map = compressor.new_state_group_map;

    // Done! Now to print a bunch of stats.

    let compressed_summed_size = new_state_group_map
        .iter()
        .fold(0, |acc, (_, v)| acc + v.state_map.len());

    let ratio = (compressed_summed_size as f64) / (original_summed_size as f64);

    println!(
        "Number of rows after compression: {} ({:.2}%)",
        compressed_summed_size,
        ratio * 100.
    );

    println!("Compression Statistics:");
    println!(
        "  Number of forced resets due to lacking prev: {}",
        compressor.stats.resets_no_suitable_prev
    );
    println!(
        "  Number of compressed rows caused by the above: {}",
        compressor.stats.resets_no_suitable_prev_size
    );
    println!(
        "  Number of state groups changed: {}",
        compressor.stats.state_groups_changed
    );

    // If we are given an output file, we output the changes as SQL. If the
    // `individual_transactions` argument is set we wrap each change to a state
    // group in a transaction.

    if let Some(output) = &mut output_file {
        for (sg, old_entry) in &state_group_map {
            let new_entry = &new_state_group_map[sg];

            if old_entry != new_entry {
                if individual_transactions {
                    writeln!(output, "BEGIN;");
                }

                writeln!(
                    output,
                    "DELETE FROM state_group_edges WHERE state_group = {};",
                    sg
                );

                if let Some(prev_sg) = new_entry.prev_state_group {
                    writeln!(output, "INSERT INTO state_group_edges (state_group, prev_state_group) VALUES ({}, {});", sg, prev_sg);
                }

                writeln!(
                    output,
                    "DELETE FROM state_groups_state WHERE state_group = {};",
                    sg
                );
                if new_entry.state_map.len() > 0 {
                    writeln!(output, "INSERT INTO state_groups_state (state_group, room_id, type, state_key, event_id) VALUES");
                    let mut first = true;
                    for ((t, s), e) in new_entry.state_map.iter() {
                        if first {
                            write!(output, "     ");
                            first = false;
                        } else {
                            write!(output, "    ,");
                        }
                        writeln!(output, "({}, '{}', '{}', '{}', '{}')", sg, room_id, t, s, e);
                    }
                    writeln!(output, ";");
                }

                if individual_transactions {
                    writeln!(output, "COMMIT;");
                }
                writeln!(output);
            }
        }
    }

    // Now let's iterate through and assert that the state for each group
    // matches between the two versions.
    state_group_map
        .par_iter() // This uses rayon to run the checks in parallel
        .try_for_each(|(sg, _)| {
            let expected = collapse_state_maps(&state_group_map, *sg);
            let actual = collapse_state_maps(&new_state_group_map, *sg);

            if expected != actual {
                println!("State Group: {}", sg);
                println!("Expected: {:#?}", expected);
                println!("actual: {:#?}", actual);
                Err(format!("State for group {} do not match", sg))
            } else {
                Ok(())
            }
        }).expect("expected state to match");

    println!("New state map matches old one");
}
