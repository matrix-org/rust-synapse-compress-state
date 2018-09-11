#[macro_use]
extern crate clap;
extern crate fallible_iterator;
extern crate postgres;
extern crate rayon;
extern crate rust_matrix_lib;

use clap::{App, Arg, ArgGroup};
use fallible_iterator::FallibleIterator;
use postgres::{Connection, TlsMode};
use rayon::prelude::*;
use rust_matrix_lib::state_map::StateMap;

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Write};

/// An entry for a state group. Consists of an (optional) previous group and the
/// delta from that previous group (or the full state if no previous group)
#[derive(Default, Debug, Clone)]
struct StateGroupEntry {
    prev_state_group: Option<i64>,
    state_map: StateMap<String>,
}

/// Fetch the entries in state_groups_state (and their prev groups) for the
/// given `room_id` by connecting to the postgres database at `db_url`.
fn get_data_from_db(db_url: &str, room_id: &str) -> BTreeMap<i64, StateGroupEntry> {
    let conn = Connection::connect(db_url, TlsMode::None).unwrap();

    let stmt = conn
        .prepare(
            r#"
        SELECT state_group, prev_state_group, type, state_key, event_id
        FROM state_groups_state
        LEFT JOIN state_group_edges USING (state_group)
        WHERE room_id = $1
    "#,
        ).unwrap();
    let trans = conn.transaction().unwrap();

    let mut rows = stmt.lazy_query(&trans, &[&room_id], 100).unwrap();

    let mut state_group_map: BTreeMap<i64, StateGroupEntry> = BTreeMap::new();

    let mut started = false;

    while let Some(row) = rows.next().unwrap() {
        if !started {
            started = true;
            println!("Started streaming from DB!");
        }
        let state_group = row.get(0);

        let entry = state_group_map.entry(state_group).or_default();

        entry.prev_state_group = row.get(1);
        entry.state_map.insert(
            &row.get::<_, String>(2),
            &row.get::<_, String>(3),
            row.get(4),
        );
    }

    state_group_map
}

/// Get any missing state groups from the database
fn get_missing_from_db(db_url: &str, missing_sgs: &[i64]) -> BTreeMap<i64, StateGroupEntry> {
    let conn = Connection::connect(db_url, TlsMode::None).unwrap();

    let stmt = conn
        .prepare(
            r#"
        SELECT state_group, prev_state_group
        FROM state_group_edges
        WHERE state_group = ANY($1)
    "#,
        ).unwrap();
    let trans = conn.transaction().unwrap();

    let mut rows = stmt.lazy_query(&trans, &[&missing_sgs], 100).unwrap();

    let mut state_group_map: BTreeMap<i64, StateGroupEntry> = BTreeMap::new();

    while let Some(row) = rows.next().unwrap() {
        let state_group = row.get(0);

        let entry = state_group_map.entry(state_group).or_default();

        entry.prev_state_group = row.get(1);
    }

    state_group_map
}

/// Get state group entries from the file at `path`.
///
/// This should be formatted as `|` separated values, with the empty string
/// representing null. (Yes, this is a bit dodgy by means its trivial to get
/// from postgres. We should use a better format).
///
/// The following invocation produces the correct output:
///
/// ```bash
/// psql -At synapse > test.data <<EOF
/// SELECT state_group, prev_state_group, type, state_key, event_id
///        FROM state_groups_state
///        LEFT JOIN state_group_edges USING (state_group)
///        WHERE room_id = '!some_room:example.com';
/// EOF
/// ```
fn get_data_from_file(path: &str) -> BTreeMap<i64, StateGroupEntry> {
    let mut state_group_map: BTreeMap<i64, StateGroupEntry> = BTreeMap::new();

    let f = File::open(path).unwrap();
    let f = BufReader::new(f);

    for line in f.lines() {
        let line = line.unwrap();

        let mut iter = line.split('|');

        let state_group = iter.next().unwrap().parse().unwrap();

        let entry = state_group_map.entry(state_group).or_default();

        let prev_state_group_str = iter.next().unwrap();
        entry.prev_state_group = if prev_state_group_str.is_empty() {
            None
        } else {
            Some(prev_state_group_str.parse().unwrap())
        };

        entry.state_map.insert(
            iter.next().unwrap(),
            iter.next().unwrap(),
            iter.next().unwrap().to_string(),
        );
    }

    state_group_map
}

/// Gets the full state for a given group from the map (of deltas)
fn collapse_state_maps(map: &BTreeMap<i64, StateGroupEntry>, state_group: i64) -> StateMap<String> {
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
                .takes_value(true),
        ).arg(
            Arg::with_name("input")
                .short("f")
                .value_name("FILE")
                .help("File containing dumped state groups")
                .takes_value(true),
        ).arg(
            Arg::with_name("output_diff")
                .short("o")
                .value_name("FILE")
                .help("File to output the changes to")
                .takes_value(true),
        ).group(
            ArgGroup::with_name("target")
                .args(&["input", "room_id"])
                .required(true),
        ).get_matches();

    let db_url = matches
        .value_of("postgres-url")
        .expect("db url should be required");

    let mut output_file = matches
        .value_of("output_diff")
        .map(|path| File::create(path).unwrap());

    // First we need to get the current state groups
    let mut state_group_map = if let Some(path) = matches.value_of("input") {
        get_data_from_file(path)
    } else {
        let room_id = matches
            .value_of("room_id")
            .expect("room_id should be required since no file");
        get_data_from_db(db_url, room_id)
    };

    // For reasons that escape me some of the state groups appear in the edges
    // table, but not in the state_groups_state table. This means they don't
    // get included in our DB queries, so we have to fetch any missing groups
    // explicitly. Since the returned groups may themselves reference groups
    // we don't have we need to do this recursively until we don't find any
    // more
    loop {
        let missing_sgs: Vec<_> = state_group_map
            .iter()
            .filter_map(|(_sg, entry)| {
                if let Some(prev_sg) = entry.prev_state_group {
                    if state_group_map.contains_key(&prev_sg) {
                        None
                    } else {
                        Some(prev_sg)
                    }
                } else {
                    None
                }
            }).collect();

        if missing_sgs.is_empty() {
            break;
        }

        println!("Missing {} state groups", missing_sgs.len());

        let map = get_missing_from_db(db_url, &missing_sgs);
        state_group_map.extend(map.into_iter());
    }

    println!("Number of entries: {}", state_group_map.len());

    let summed_size = state_group_map
        .iter()
        .fold(0, |acc, (_, v)| acc + v.state_map.len());

    println!("Number of rows: {}", summed_size);

    let mut new_state_group_map: BTreeMap<i64, StateGroupEntry> = BTreeMap::new();

    // Now we loop through and create our new state maps from the existing
    // ones.
    // The existing table is made up of chains of groups at most 100 nodes
    // long. At the start of each chain there is a copy of the full state at
    // that point. This algorithm adds edges between such "checkpoint" nodes,
    // so that there are chains between them. We cap such checkpoint chains to
    // a length of 50.
    //
    // The idea here is that between checkpoint nodes only small subsets of
    // state will have actually changed.
    //
    // (This approach can be generalised by adding more and more layers)

    let mut last_checkpoint_opt = None;
    let mut checkpoint_length = 0;

    for (state_group, entry) in &state_group_map {
        if entry.prev_state_group.is_none() {
            // We're at a checkpoint node. If this is our first checkpoint
            // node then there isn't much to do other than mark it.
            let mut added_to_chain = false;
            if let Some(ref last_checkpoint) = last_checkpoint_opt {
                let checkpoint_entry = &state_group_map[last_checkpoint];

                // We need to ensure that that aren't any entries in the
                // previous checkpoint node that aren't in the state at this
                // point, since the table schema doesn't support the idea of
                // "deleting" state in the deltas.
                //
                // Note: The entry.state_map will be the full state here, rather
                // than just the delta since prev_state_group is None.
                if checkpoint_entry
                    .state_map
                    .keys()
                    .all(|(t, s)| entry.state_map.contains_key(t, s))
                {
                    // We create the new map by filtering out entries that match
                    // those in the previous checkpoint state.
                    let new_map: StateMap<String> = entry
                        .state_map
                        .iter()
                        .filter(|((t, s), e)| checkpoint_entry.state_map.get(t, s) != Some(e))
                        .map(|((t, s), e)| ((t, s), e.clone()))
                        .collect();

                    // If we have an output file write the changes we've made
                    if let Some(ref mut fs) = output_file {
                        writeln!(fs, "edge_addition {} {}", state_group, *last_checkpoint).unwrap();
                        for ((t, s), e) in new_map.iter() {
                            writeln!(fs, "state_replace {} {} {} {}", state_group, t, s, e)
                                .unwrap();
                        }
                    }

                    new_state_group_map.insert(
                        *state_group,
                        StateGroupEntry {
                            prev_state_group: Some(*last_checkpoint),
                            state_map: new_map,
                        },
                    );

                    added_to_chain = true;
                } else {
                    new_state_group_map.insert(*state_group, entry.clone());
                }
            } else {
                new_state_group_map.insert(*state_group, entry.clone());
            }

            last_checkpoint_opt = Some(*state_group);

            // If we've added to the checkpoint chain we increment the length,
            // otherwise it gets reset to zero.
            if added_to_chain {
                checkpoint_length += 1;
            } else {
                checkpoint_length = 0;
            }

            // If the chain is longer than 50 then lets reset to create a new
            // chain.
            if checkpoint_length >= 50 {
                checkpoint_length = 0;
                last_checkpoint_opt = None;
            }
        } else {
            new_state_group_map.insert(*state_group, entry.clone());
        }
    }

    let summed_size = new_state_group_map
        .iter()
        .fold(0, |acc, (_, v)| acc + v.state_map.len());

    println!("Number of rows compressed: {}", summed_size);

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
