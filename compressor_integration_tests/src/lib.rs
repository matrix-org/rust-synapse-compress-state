use log::LevelFilter;
use openssl::ssl::{SslConnector, SslMethod, SslVerifyMode};
use postgres::{fallible_iterator::FallibleIterator, Client};
use postgres_openssl::MakeTlsConnector;
use rand::{distributions::Alphanumeric, thread_rng, Rng};
use state_map::StateMap;
use std::{borrow::Cow, collections::BTreeMap, env, fmt};
use string_cache::DefaultAtom as Atom;

use synapse_compress_state::StateGroupEntry;

pub mod map_builder;

pub static DB_URL: &str = "postgresql://synapse_user:synapse_pass@localhost/synapse";

/// Adds the contents of a state group map to the testing database
pub fn add_contents_to_database(room_id: &str, state_group_map: &BTreeMap<i64, StateGroupEntry>) {
    // connect to the database
    let mut builder = SslConnector::builder(SslMethod::tls()).unwrap();
    builder.set_verify(SslVerifyMode::NONE);
    let connector = MakeTlsConnector::new(builder.build());

    let mut client = Client::connect(DB_URL, connector).unwrap();

    // build up the query
    let mut sql = "".to_string();

    for (sg, entry) in state_group_map {
        // create the entry for state_groups
        sql.push_str(&format!(
            "INSERT INTO state_groups (id, room_id, event_id) VALUES ({},{},{});\n",
            sg,
            PGEscape(room_id),
            PGEscape("left_blank")
        ));

        // create the entry in state_group_edges IF exists
        if let Some(prev_sg) = entry.prev_state_group {
            sql.push_str(&format!(
                "INSERT INTO state_group_edges (state_group, prev_state_group) VALUES ({}, {});\n",
                sg, prev_sg
            ));
        }

        // write entry for each row in delta
        if !entry.state_map.is_empty() {
            sql.push_str("INSERT INTO state_groups_state (state_group, room_id, type, state_key, event_id) VALUES");

            let mut first = true;
            for ((t, s), e) in entry.state_map.iter() {
                if first {
                    sql.push_str("     ");
                    first = false;
                } else {
                    sql.push_str("    ,");
                }
                sql.push_str(&format!(
                    "({}, {}, {}, {}, {})",
                    sg,
                    PGEscape(room_id),
                    PGEscape(t),
                    PGEscape(s),
                    PGEscape(e)
                ));
            }
            sql.push_str(";\n");
        }
    }

    client.batch_execute(&sql).unwrap();
}

/// Clears the contents of the testing database
pub fn empty_database() {
    // connect to the database
    let mut builder = SslConnector::builder(SslMethod::tls()).unwrap();
    builder.set_verify(SslVerifyMode::NONE);
    let connector = MakeTlsConnector::new(builder.build());

    let mut client = Client::connect(DB_URL, connector).unwrap();

    // delete all the contents from all three tables
    let sql = r"
        TRUNCATE state_groups;
        TRUNCATE state_group_edges;
        TRUNCATE state_groups_state;
    ";

    client.batch_execute(sql).unwrap();
}

/// Safely escape the strings in sql queries
struct PGEscape<'a>(pub &'a str);

impl<'a> fmt::Display for PGEscape<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut delim = Cow::from("$$");
        while self.0.contains(&delim as &str) {
            let s: String = thread_rng()
                .sample_iter(&Alphanumeric)
                .take(10)
                .map(char::from)
                .collect();

            delim = format!("${}$", s).into();
        }

        write!(f, "{}{}{}", delim, self.0, delim)
    }
}

/// Checks whether the state at each state group is the same as what the map thinks it should be
///
/// i.e. when synapse tries to work out the state for a given state group by looking at
/// the database. Will the state it gets be the same as what the map thinks it should be
pub fn database_collapsed_states_match_map(
    state_group_map: &BTreeMap<i64, StateGroupEntry>,
) -> bool {
    for sg in state_group_map.keys() {
        let map_state = collapse_state_with_map(state_group_map, *sg);
        let database_state = collapse_state_with_database(*sg);
        if map_state != database_state {
            println!("database state {} doesn't match", sg);
            println!("expected {:?}", map_state);
            println!("but found {:?}", database_state);
            return false;
        }
    }
    true
}

/// Gets the full state for a given group from the map (of deltas)
fn collapse_state_with_map(
    map: &BTreeMap<i64, StateGroupEntry>,
    state_group: i64,
) -> StateMap<Atom> {
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
            map[sg]
                .state_map
                .iter()
                .map(|((t, s), e)| ((t, s), e.clone())),
        );
    }

    state_map
}

fn collapse_state_with_database(state_group: i64) -> StateMap<Atom> {
    // connect to the database
    let mut builder = SslConnector::builder(SslMethod::tls()).unwrap();
    builder.set_verify(SslVerifyMode::NONE);
    let connector = MakeTlsConnector::new(builder.build());

    let mut client = Client::connect(DB_URL, connector).unwrap();

    // Gets the delta for a specific state group
    let query_deltas = r#"
        SELECT m.id, type, state_key, s.event_id
        FROM state_groups AS m
        LEFT JOIN state_groups_state AS s ON (m.id = s.state_group)
        WHERE m.id = $1
    "#;

    // If there is no delta for that specific state group, then we still want to find
    // the predecessor (so have split this into a different query)
    let query_pred = r#"
        SELECT prev_state_group
        FROM state_group_edges 
        WHERE state_group = $1
    "#;

    let mut state_map: StateMap<Atom> = StateMap::new();

    let mut next_group = Some(state_group);

    while let Some(sg) = next_group {
        // get predecessor from state_group_edges
        let mut pred = client.query_raw(query_pred, &[sg]).unwrap();

        // set next_group to predecessor
        next_group = match pred.next().unwrap() {
            Some(p) => p.get(0),
            None => None,
        };

        // if there was a predecessor then assert that it is unique
        if next_group.is_some() {
            assert!(pred.next().unwrap().is_none());
        }
        drop(pred);

        let mut rows = client.query_raw(query_deltas, &[sg]).unwrap();

        while let Some(row) = rows.next().unwrap() {
            // Copy the single delta from the predecessor stored in this row
            if let Some(etype) = row.get::<_, Option<String>>(1) {
                let key = &row.get::<_, String>(2);

                // only insert if not overriding existing entry
                // this is because the newer delta is found FIRST
                if state_map.get(&etype, key).is_none() {
                    state_map.insert(&etype, key, row.get::<_, String>(3).into());
                }
            }
        }
    }

    state_map
}

/// Check whether predecessors and deltas stored in the database are the same as in the map
pub fn database_structure_matches_map(state_group_map: &BTreeMap<i64, StateGroupEntry>) -> bool {
    // connect to the database
    let mut builder = SslConnector::builder(SslMethod::tls()).unwrap();
    builder.set_verify(SslVerifyMode::NONE);
    let connector = MakeTlsConnector::new(builder.build());

    let mut client = Client::connect(DB_URL, connector).unwrap();

    // Gets the delta for a specific state group
    let query_deltas = r#"
        SELECT m.id, type, state_key, s.event_id
        FROM state_groups AS m
        LEFT JOIN state_groups_state AS s ON (m.id = s.state_group)
        WHERE m.id = $1
    "#;

    // If there is no delta for that specific state group, then we still want to find
    // the predecessor (so have split this into a different query)
    let query_pred = r#"
        SELECT prev_state_group
        FROM state_group_edges 
        WHERE state_group = $1
    "#;

    for (sg, entry) in state_group_map {
        // get predecessor from state_group_edges
        let mut pred_iter = client.query_raw(query_pred, &[sg]).unwrap();

        // read out the predecessor value from the database
        let database_pred = match pred_iter.next().unwrap() {
            Some(p) => p.get(0),
            None => None,
        };

        // if there was a predecessor then assert that it is unique
        if database_pred.is_some() {
            assert!(pred_iter.next().unwrap().is_none());
        }

        // check if it matches map
        if database_pred != entry.prev_state_group {
            println!(
                "ERROR: predecessor for {} was {:?} (expected {:?})",
                sg, database_pred, entry.prev_state_group
            );
            return false;
        }
        // needed so that can create another query
        drop(pred_iter);

        // Now check that deltas are the same
        let mut state_map: StateMap<Atom> = StateMap::new();

        // Get delta from state_groups_state
        let mut rows = client.query_raw(query_deltas, &[sg]).unwrap();

        while let Some(row) = rows.next().unwrap() {
            // Copy the single delta from the predecessor stored in this row
            if let Some(etype) = row.get::<_, Option<String>>(1) {
                state_map.insert(
                    &etype,
                    &row.get::<_, String>(2),
                    row.get::<_, String>(3).into(),
                );
            }
        }

        // Check that the delta matches the map
        if state_map != entry.state_map {
            println!("ERROR: delta for {} didn't match", sg);
            println!("Expected: {:?}", entry.state_map);
            println!("Actual: {:?}", state_map);
            return false;
        }
    }
    true
}

/// Clears the compressor state from the database
pub fn clear_compressor_state() {
    // connect to the database
    let mut builder = SslConnector::builder(SslMethod::tls()).unwrap();
    builder.set_verify(SslVerifyMode::NONE);
    let connector = MakeTlsConnector::new(builder.build());

    let mut client = Client::connect(DB_URL, connector).unwrap();

    // delete all the contents from the state compressor tables
    let sql = r"
        TRUNCATE state_compressor_state;
        TRUNCATE state_compressor_progress;
        UPDATE state_compressor_total_progress SET lowest_uncompressed_group = 0;
    ";

    client.batch_execute(sql).unwrap();
}

#[test]
fn functions_are_self_consistent() {
    let mut initial: BTreeMap<i64, StateGroupEntry> = BTreeMap::new();
    let mut prev = None;

    // This starts with the following structure
    //
    // 0-1-2-3-4-5-6-7-8-9-10-11-12-13
    //
    // Each group i has state:
    //     ('node','is',      i)
    //     ('group',  j, 'seen') - for all j less than i
    for i in 0i64..=13i64 {
        let mut entry = StateGroupEntry {
            in_range: true,
            prev_state_group: prev,
            state_map: StateMap::new(),
        };
        entry
            .state_map
            .insert("group", &i.to_string(), "seen".into());
        entry.state_map.insert("node", "is", i.to_string().into());

        initial.insert(i, entry);

        prev = Some(i)
    }

    empty_database();
    add_contents_to_database("room1", &initial);

    assert!(database_collapsed_states_match_map(&initial));
    assert!(database_structure_matches_map(&initial));
}

pub fn setup_logger() {
    // setup the logger for the auto_compressor
    // The default can be overwritten with RUST_LOG
    // see the README for more information
    if env::var("RUST_LOG").is_err() {
        let mut log_builder = env_logger::builder();
        // set is_test(true) so that the output is hidden by cargo test (unless the test fails)
        log_builder.is_test(true);
        // default to printing the debug information for both packages being tested
        // (Note that just setting the global level to debug will log every sql transaction)
        log_builder.filter_module("synapse_compress_state", LevelFilter::Debug);
        log_builder.filter_module("auto_compressor", LevelFilter::Debug);
        // use try_init() incase the logger has been setup by some previous test
        let _ = log_builder.try_init();
    } else {
        // If RUST_LOG was set then use that
        let mut log_builder = env_logger::Builder::from_env("RUST_LOG");
        // set is_test(true) so that the output is hidden by cargo test (unless the test fails)
        log_builder.is_test(true);
        // use try_init() in case the logger has been setup by some previous test
        let _ = log_builder.try_init();
    }
}
