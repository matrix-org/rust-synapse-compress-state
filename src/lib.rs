// Copyright 2018 New Vector Ltd
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! This is a tool that attempts to further compress state maps within a
//! Synapse instance's database. Specifically, it aims to reduce the number of
//! rows that a given room takes up in the `state_groups_state` table.

// This file contains configuring config options, which neccessarily means lots
// of arguments - this hopefully doesn't make the code unclear
#![allow(clippy::too_many_arguments)]

use pyo3::prelude::*;

#[global_allocator]
static GLOBAL: jemallocator::Jemalloc = jemallocator::Jemalloc;

use clap::{
    crate_authors, crate_description, crate_name, crate_version, value_t_or_exit, App, Arg,
};
use indicatif::{ProgressBar, ProgressStyle};
use rayon::prelude::*;
use state_map::StateMap;
use std::{collections::BTreeMap, fs::File, io::Write, str::FromStr};
use string_cache::DefaultAtom as Atom;

mod compressor;
mod database;
mod graphing;

use compressor::Compressor;
use database::PGEscape;

/// An entry for a state group. Consists of an (optional) previous group and the
/// delta from that previous group (or the full state if no previous group)
#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub struct StateGroupEntry {
    in_range: bool,
    prev_state_group: Option<i64>,
    state_map: StateMap<Atom>,
}

/// Helper struct for parsing the `level_sizes` argument.
struct LevelSizes(Vec<usize>);

impl FromStr for LevelSizes {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut sizes = Vec::new();

        for size_str in s.split(',') {
            let size: usize = size_str
                .parse()
                .map_err(|_| "Not a comma separated list of numbers")?;
            sizes.push(size);
        }

        Ok(LevelSizes(sizes))
    }
}

/// Contains configuration information for this run of the compressor
pub struct Config {
    // the url for the postgres database
    // this should be of the form postgres://user:pass@domain/database
    db_url: String,
    // The file where the transactions are written that would carry out
    // the compression that get's calculated
    output_file: Option<File>,
    // The ID of the room who's state is being compressed
    room_id: String,
    // The group to start compressing from
    // N.B. THIS STATE ITSELF IS NOT COMPRESSED!!!
    // Note there is no state 0 so if want to start
    min_state_group: Option<i64>,
    // How many groups to do the compression on
    // Note: State groups within the range specified will get compressed
    // if they are in the state_groups table. States that only appear in
    // the edges table MIGHT NOT get compressed - it is assumed that these
    // groups have no associated state. (Note that this was also an assumption
    // in previous versions of the state compressor)
    groups_to_compress: Option<i64>,
    // If the compressor results in less than this many rows being saved then 
    // it will abort
    min_saved_rows: Option<i32>,
    // The sizes of the different levels in the new state_group tree being built
    level_sizes: LevelSizes,
    // Whether or not to wrap each change to an individual state_group in a transaction
    // This is very much reccomended when running the compression when synapse is live
    // TODO: should this actually be an opt-out flag? it's much worse to need it and
    //       forget to add it than to not need it and forget to remove it....?
    transactions: bool,
    // Whether or not to output before and after directed graphs (these can be
    // visualised in somthing like Gephi)
    graphs: bool,
}

impl Config {
    /// Build up config from command line arguments
    pub fn parse_arguments() -> Config {
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
            Arg::with_name("min_state_group")
                .short("s")
                .value_name("MIN_STATE_GROUP")
                .help("The state group to start processing from (non inclusive)")
                .takes_value(true)
                .required(false),
        ).arg(
            Arg::with_name("min_saved_rows")
            .short("m")
            .value_name("COUNT")
            .help("Abort if fewer than COUNT rows would be saved")
            .takes_value(true)
            .required(false),
        ).arg(
            Arg::with_name("groups_to_compress")
                .short("n")
                .value_name("GROUPS_TO_COMPRESS")
                .help("How many groups to load into memory to compress")
                .takes_value(true)
                .required(false),
        ).arg(
            Arg::with_name("output_file")
                .short("o")
                .value_name("FILE")
                .help("File to output the changes to in SQL")
                .takes_value(true),
        ).arg(
            Arg::with_name("level_sizes")
                .short("l")
                .value_name("LEVELS")
                .help("Sizes of each new level in the compression algorithm, as a comma separated list.")
                .long_help(concat!(
                    "Sizes of each new level in the compression algorithm, as a comma separated list.",
                    " The first entry in the list is for the lowest, most granular level,",
                    " with each subsequent entry being for the next highest level.",
                    " The number of entries in the list determines the number of levels",
                    " that will be used.",
                    "\nThe sum of the sizes of the levels effect the performance of fetching the state",
                    " from the database, as the sum of the sizes is the upper bound on number of",
                    " iterations needed to fetch a given set of state.",
                ))
                .default_value("100,50,25")
                .takes_value(true),
        ).arg(
            Arg::with_name("transactions")
                .short("t")
                .help("Whether to wrap each state group change in a transaction")
                .requires("output_file"),
        ).arg(
            Arg::with_name("graphs")
                .short("g")
                .help("Whether to produce graphs of state groups before and after compression instead of SQL")
        ).get_matches();

        let db_url = matches
            .value_of("postgres-url")
            .expect("db url should be required");

        let output_file = matches
            .value_of("output_file")
            .map(|path| File::create(path).unwrap());

        let room_id = matches
            .value_of("room_id")
            .expect("room_id should be required since no file");

        let min_state_group = matches
            .value_of("min_state_group")
            .map(|s| s.parse().expect("min_state_group must be an integer"));

        let groups_to_compress = matches
            .value_of("groups_to_compress")
            .map(|s| s.parse().expect("groups_to_compress must be an integer"));

        let min_saved_rows = matches
            .value_of("min_saved_rows")
            .map(|v| v.parse().expect("COUNT must be an integer"));

        let level_sizes = value_t_or_exit!(matches, "level_sizes", LevelSizes);

        let transactions = matches.is_present("transactions");

        let graphs = matches.is_present("graphs");

        Config {
            db_url: String::from(db_url),
            output_file,
            room_id: String::from(room_id),
            min_state_group,
            groups_to_compress,
            min_saved_rows,
            level_sizes,
            transactions,
            graphs,
        }
    }
}

/// Runs through the steps of the compression:
///
/// - Fetches current state groups for a room and their predecessors
/// - Outputs #state groups and #lines in table they occupy
/// - Runs the compressor to produce a new predecessor mapping
/// - Outputs #lines in table that the new mapping would occupy
/// - Outputs info about how the compressor got on
/// - Checks that number of lines saved is greater than threshold
/// - Ensures new mapping doesn't affect actual state contents
/// - Produces SQL code to carry out changes and saves it to file
///
/// # Arguments
///
/// * `config: Config` - A Config struct that controlls the run

pub fn run(mut config: Config) {
    // First we need to get the current state groups
    println!("Fetching state from DB for room '{}'...", config.room_id);

    let (state_group_map, max_group_found) = database::get_data_from_db(
        &config.db_url,
        &config.room_id,
        config.min_state_group,
        config.groups_to_compress,
    );
    println!("Fetched state groups up to {}", max_group_found);

    println!("Number of state groups: {}", state_group_map.len());

    let original_summed_size = state_group_map
        .iter()
        .fold(0, |acc, (_, v)| acc + v.state_map.len());

    println!("Number of rows in current table: {}", original_summed_size);

    // Now we actually call the compression algorithm.

    println!("Compressing state...");

    let compressor = Compressor::compress(&state_group_map, &config.level_sizes.0);

    let new_state_group_map = &compressor.new_state_group_map;

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

    if let Some(min) = config.min_saved_rows {
        let saving = (original_summed_size - compressed_summed_size) as i32;
        if saving < min {
            println!(
                "Only {} rows would be saved by this compression. Skipping output.",
                saving
            );
            return;
        }
    }

    check_that_maps_match(&state_group_map, &new_state_group_map);

    // If we are given an output file, we output the changes as SQL. If the
    // `transactions` argument is set we wrap each change to a state group in a
    // transaction.

    output_sql(&mut config, &state_group_map, &new_state_group_map);

    if config.graphs {
        graphing::make_graphs(&state_group_map, &new_state_group_map);
    }
}

/// Produces SQL code to carry out changes and saves it to file
///
/// # Arguments
///
/// * `config` -    A Config struct that contains information
///                 about the run. It's mutable because it contains
///                 the pointer to the output file (which needs to
///                 be mutable for the file to be written to)
/// * `old_map` -   The state group data originally in the database
/// * `new_map` -   The state group data generated by the compressor to
///                 replace replace the old contents
fn output_sql(
    config: &mut Config,
    old_map: &BTreeMap<i64, StateGroupEntry>,
    new_map: &BTreeMap<i64, StateGroupEntry>,
) {
    if config.output_file.is_none() {
        return;
    }

    println!("Writing changes...");

    let pb = ProgressBar::new(old_map.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar().template("[{elapsed_precise}] {bar} {pos}/{len} {msg}"),
    );
    pb.set_message("state groups");
    pb.enable_steady_tick(100);

    if let Some(output) = &mut config.output_file {
        for (sg, old_entry) in old_map {
            let new_entry = &new_map[sg];

            // N.B. also checks if in_range fields agree
            if old_entry != new_entry {
                if config.transactions {
                    writeln!(output, "BEGIN;").unwrap();
                }
                writeln!(
                    output,
                    "DELETE FROM state_group_edges WHERE state_group = {};",
                    sg
                )
                .unwrap();

                if let Some(prev_sg) = new_entry.prev_state_group {
                    writeln!(output, "INSERT INTO state_group_edges (state_group, prev_state_group) VALUES ({}, {});", sg, prev_sg).unwrap();
                }

                writeln!(
                    output,
                    "DELETE FROM state_groups_state WHERE state_group = {};",
                    sg
                )
                .unwrap();
                if !new_entry.state_map.is_empty() {
                    writeln!(output, "INSERT INTO state_groups_state (state_group, room_id, type, state_key, event_id) VALUES").unwrap();
                    let mut first = true;
                    for ((t, s), e) in new_entry.state_map.iter() {
                        if first {
                            write!(output, "     ").unwrap();
                            first = false;
                        } else {
                            write!(output, "    ,").unwrap();
                        }
                        writeln!(
                            output,
                            "({}, {}, {}, {}, {})",
                            sg,
                            PGEscape(&config.room_id),
                            PGEscape(t),
                            PGEscape(s),
                            PGEscape(e)
                        )
                        .unwrap();
                    }
                    writeln!(output, ";").unwrap();
                }

                if config.transactions {
                    writeln!(output, "COMMIT;").unwrap();
                }
                writeln!(output).unwrap();
            }

            pb.inc(1);
        }
    }

    pb.finish();
}

/// Compares two sets of state groups
///
/// A state group entry contains a predecessor state group and a delta.
/// The complete contents of a certain state group can be calculated by
/// following this chain of predecessors back to some empty state and
/// combining all the deltas together. This is called "collapsing".
///
/// This function confirms that two state groups mappings lead to the
/// exact same entries for each state group after collapsing them down.
///
/// # Arguments
/// * `old_map` -   The state group data currently in the database
/// * `new_map` -   The state group data that the old_map is being compared
///                 to
fn check_that_maps_match(
    old_map: &BTreeMap<i64, StateGroupEntry>,
    new_map: &BTreeMap<i64, StateGroupEntry>,
) {
    println!("Checking that state maps match...");

    let pb = ProgressBar::new(old_map.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar().template("[{elapsed_precise}] {bar} {pos}/{len} {msg}"),
    );
    pb.set_message("state groups");
    pb.enable_steady_tick(100);

    // Now let's iterate through and assert that the state for each group
    // matches between the two versions.
    old_map
        .par_iter() // This uses rayon to run the checks in parallel
        .try_for_each(|(sg, _)| {
            let expected = collapse_state_maps(&old_map, *sg);
            let actual = collapse_state_maps(&new_map, *sg);

            pb.inc(1);

            if expected != actual {
                println!("State Group: {}", sg);
                println!("Expected: {:#?}", expected);
                println!("actual: {:#?}", actual);
                Err(format!("State for group {} do not match", sg))
            } else {
                Ok(())
            }
        })
        .expect("expected state to match");

    pb.finish();

    println!("New state map matches old one");
}

/// Gets the full state for a given group from the map (of deltas)
fn collapse_state_maps(map: &BTreeMap<i64, StateGroupEntry>, state_group: i64) -> StateMap<Atom> {
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

// PyO3 INTERFACE STARTS HERE

impl Config {
    /// Converts string and bool arguments into a Config struct
    pub fn new(
        db_url: String,
        output_file: String,
        room_id: String,
        min_state_group: String,
        groups_to_compress: String,
        min_saved_rows: String,
        level_sizes: String,
        transactions: bool,
        graphs: bool,
    ) -> Config {
        if db_url.is_empty() {
            panic!("db url is required");
        }

        let mut output: Option<File> = None;
        if !output_file.is_empty() {
            output = Some(File::create(output_file).unwrap());
        }
        let output_file = output;

        if room_id.is_empty() {
            panic!("room_id is required");
        }

        let mut min_row: Option<i64> = None;
        if !min_state_group.is_empty() {
            min_row = Some(min_state_group.parse().unwrap());
        }
        let min_state_group = min_row;

        let mut num_groups: Option<i64> = None;
        if !groups_to_compress.is_empty() {
            num_groups = Some(groups_to_compress.parse().unwrap());
        }
        let groups_to_compress = num_groups;

        let mut min_count: Option<i32> = None;
        if !min_saved_rows.is_empty() {
            min_count = Some(min_saved_rows.parse().unwrap());
        }
        let min_saved_rows = min_count;

        let level_sizes: LevelSizes = level_sizes.parse().unwrap();

        Config {
            db_url,
            output_file,
            room_id,
            min_state_group,
            groups_to_compress,
            min_saved_rows,
            level_sizes,
            transactions,
            graphs,
        }
    }
}

/// Access point for python code
///
/// Default arguments are equivalent to using the command line tool
#[pyfunction(
    db_url = "String::from(\"\")",
    output_file = "String::from(\"\")",
    room_id = "String::from(\"\")",
    min_state_group = "String::from(\"\")",
    groups_to_compress = "String::from(\"\")",
    min_saved_rows = "String::from(\"\")",
    level_sizes = "String::from(\"100,50,25\")",
    // have this default to true as is much worse to not have it if you need it
    // than to have it and not need it
    transactions = true,
    graphs = false
)]
fn run_compression(
    db_url: String,
    output_file: String,
    room_id: String,
    min_state_group: String,
    groups_to_compress: String,
    min_saved_rows: String,
    level_sizes: String,
    transactions: bool,
    graphs: bool,
) {
    let config = Config::new(
        db_url,
        output_file,
        room_id,
        min_state_group,
        groups_to_compress,
        min_saved_rows,
        level_sizes,
        transactions,
        graphs,
    );
    run(config);
}

/// Python module - "import synapse_compress_state" to use
#[pymodule]
fn synapse_compress_state(_py: Python, m: &PyModule) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(run_compression, m)?)?;
    Ok(())
}
