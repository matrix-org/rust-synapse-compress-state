use std::collections::BTreeMap;

use serial_test::serial;
use state_map::StateMap;
use synapse_compress_state::{run, Config, StateGroupEntry};

use crate::common::{database_collapsed_states_match_map, database_structure_matches_map, DB_URL};

mod common;

// Remember to add #[serial(db)] before any test that access the database.
// Only one test with this annotation can run at once - preventing
// concurrency bugs.
//
// You will probably also want to use common::empty_database() at the start
// of each test as well (since their order of execution is not guaranteed)

#[test]
#[serial(db)]
fn run_succeeds_without_crashing() {
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

    common::empty_database();
    common::add_contents_to_database("room1", &initial);

    let db_url = DB_URL.to_string();
    let output_file = "./tests/tmp/run_succeeds_without_crashing.sql".to_string();
    let room_id = "room1".to_string();
    let min_state_group = "".to_string();
    let min_saved_rows = "".to_string();
    let groups_to_compress = "".to_string();
    let level_sizes = "3,3".to_string();
    let transactions = true;
    let graphs = false;
    let commit_changes = false;

    let config = Config::new(
        db_url.clone(),
        output_file,
        room_id.clone(),
        min_state_group,
        groups_to_compress,
        min_saved_rows,
        level_sizes,
        transactions,
        graphs,
        commit_changes,
    );

    run(config);
}

#[test]
#[serial(db)]
fn changes_commited_if_no_min_saved_rows() {
    let mut initial: BTreeMap<i64, StateGroupEntry> = BTreeMap::new();
    let mut prev = None;

    // This starts with the following structure
    //
    // 0-1-2 3-4-5 6-7-8 9-10-11 12-13
    //
    // Each group i has state:
    //     ('node','is',      i)
    //     ('group',  j, 'seen') - for all j less than i
    for i in 0i64..=13i64 {
        // if the state is a snapshot then set its predecessor to NONE
        if [0, 3, 6, 9, 12].contains(&i) {
            prev = None;
        }

        // create a blank entry for it
        let mut entry = StateGroupEntry {
            in_range: true,
            prev_state_group: prev,
            state_map: StateMap::new(),
        };

        // if it's a snapshot then add in all previous state
        if prev.is_none() {
            for j in 0i64..i {
                entry
                    .state_map
                    .insert("group", &j.to_string(), "seen".into());
            }
        }

        // add in the new state for this state group
        entry
            .state_map
            .insert("group", &i.to_string(), "seen".into());
        entry.state_map.insert("node", "is", i.to_string().into());

        // put it into the initial map
        initial.insert(i, entry);

        // set this group as the predecessor for the next
        prev = Some(i)
    }

    // Place this initial state into an empty database
    common::empty_database();
    common::add_contents_to_database("room1", &initial);

    // set up the config options
    let db_url = DB_URL.to_string();
    let output_file = "./tests/tmp/changes_commited_if_no_min_saved_rows.sql".to_string();
    let room_id = "room1".to_string();
    let min_state_group = "".to_string();
    let min_saved_rows = "".to_string();
    let groups_to_compress = "".to_string();
    let level_sizes = "3,3".to_string();
    let transactions = true;
    let graphs = false;
    let commit_changes = true;

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
        commit_changes,
    );

    // Run the compressor with those settings
    run(config);

    // This should have created the following structure in the database
    // i.e. groups 6 and 9 should have changed from before
    // N.B. this saves 11 rows
    //
    // 0  3\      12
    // 1  4 6\    13
    // 2  5 7 9
    //      8 10
    //        11
    let expected_edges: BTreeMap<i64, i64> = vec![
        (1, 0),
        (2, 1),
        (4, 3),
        (5, 4),
        (6, 3),
        (7, 6),
        (8, 7),
        (9, 6),
        (10, 9),
        (11, 10),
        (13, 12),
    ]
    .into_iter()
    .collect();

    let mut expected: BTreeMap<i64, StateGroupEntry> = BTreeMap::new();

    // Each group i has state:
    //     ('node','is',      i)
    //     ('group',  j, 'seen') - for all j less than i
    for i in 0i64..=13i64 {
        let prev = expected_edges.get(&i);

        //change from Option<&i64> to Option<i64>
        let prev = match prev {
            Some(p) => Some(*p),
            None => None,
        };

        // create a blank entry for it
        let mut entry = StateGroupEntry {
            in_range: true,
            prev_state_group: prev,
            state_map: StateMap::new(),
        };

        // Add in all state between predecessor and now (non inclusive)
        if let Some(p) = prev {
            for j in (p + 1)..i {
                entry
                    .state_map
                    .insert("group", &j.to_string(), "seen".into());
            }
        } else {
            for j in 0i64..i {
                entry
                    .state_map
                    .insert("group", &j.to_string(), "seen".into());
            }
        }

        // add in the new state for this state group
        entry
            .state_map
            .insert("group", &i.to_string(), "seen".into());
        entry.state_map.insert("node", "is", i.to_string().into());

        // put it into the expected map
        expected.insert(i, entry);
    }

    // Check that the database still gives correct states for each group!
    assert!(database_collapsed_states_match_map(&initial));

    // Check that the structure of the database matches the expected structure
    assert!(database_structure_matches_map(&expected))
}

#[test]
#[serial(db)]
fn changes_commited_if_min_saved_rows_exceeded() {
    let mut initial: BTreeMap<i64, StateGroupEntry> = BTreeMap::new();
    let mut prev = None;

    // This starts with the following structure
    //
    // 0-1-2 3-4-5 6-7-8 9-10-11 12-13
    //
    // Each group i has state:
    //     ('node','is',      i)
    //     ('group',  j, 'seen') - for all j less than i
    for i in 0i64..=13i64 {
        // if the state is a snapshot then set its predecessor to NONE
        if [0, 3, 6, 9, 12].contains(&i) {
            prev = None;
        }

        // create a blank entry for it
        let mut entry = StateGroupEntry {
            in_range: true,
            prev_state_group: prev,
            state_map: StateMap::new(),
        };

        // if it's a snapshot then add in all previous state
        if prev.is_none() {
            for j in 0i64..i {
                entry
                    .state_map
                    .insert("group", &j.to_string(), "seen".into());
            }
        }

        // add in the new state for this state group
        entry
            .state_map
            .insert("group", &i.to_string(), "seen".into());
        entry.state_map.insert("node", "is", i.to_string().into());

        // put it into the initial map
        initial.insert(i, entry);

        // set this group as the predecessor for the next
        prev = Some(i)
    }

    // Place this initial state into an empty database
    common::empty_database();
    common::add_contents_to_database("room1", &initial);

    // set up the config options
    let db_url = DB_URL.to_string();
    let output_file = "./tests/tmp/changes_commited_if_no_min_saved_rows.sql".to_string();
    let room_id = "room1".to_string();
    let min_state_group = "".to_string();
    let min_saved_rows = "10".to_string();
    let groups_to_compress = "".to_string();
    let level_sizes = "3,3".to_string();
    let transactions = true;
    let graphs = false;
    let commit_changes = true;

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
        commit_changes,
    );

    // Run the compressor with those settings
    run(config);

    // This should have created the following structure in the database
    // i.e. groups 6 and 9 should have changed from before
    // N.B. this saves 11 rows
    //
    // 0  3\      12
    // 1  4 6\    13
    // 2  5 7 9
    //      8 10
    //        11
    let expected_edges: BTreeMap<i64, i64> = vec![
        (1, 0),
        (2, 1),
        (4, 3),
        (5, 4),
        (6, 3),
        (7, 6),
        (8, 7),
        (9, 6),
        (10, 9),
        (11, 10),
        (13, 12),
    ]
    .into_iter()
    .collect();

    let mut expected: BTreeMap<i64, StateGroupEntry> = BTreeMap::new();

    // Each group i has state:
    //     ('node','is',      i)
    //     ('group',  j, 'seen') - for all j less than i
    for i in 0i64..=13i64 {
        let prev = expected_edges.get(&i);

        //change from Option<&i64> to Option<i64>
        let prev = match prev {
            Some(p) => Some(*p),
            None => None,
        };

        // create a blank entry for it
        let mut entry = StateGroupEntry {
            in_range: true,
            prev_state_group: prev,
            state_map: StateMap::new(),
        };

        // Add in all state between predecessor and now (non inclusive)
        if let Some(p) = prev {
            for j in (p + 1)..i {
                entry
                    .state_map
                    .insert("group", &j.to_string(), "seen".into());
            }
        } else {
            for j in 0i64..i {
                entry
                    .state_map
                    .insert("group", &j.to_string(), "seen".into());
            }
        }

        // add in the new state for this state group
        entry
            .state_map
            .insert("group", &i.to_string(), "seen".into());
        entry.state_map.insert("node", "is", i.to_string().into());

        // put it into the expected map
        expected.insert(i, entry);
    }

    // Check that the database still gives correct states for each group!
    assert!(database_collapsed_states_match_map(&initial));

    // Check that the structure of the database matches the expected structure
    assert!(database_structure_matches_map(&expected));
}

#[test]
#[serial(db)]
fn changes_not_commited_if_fewer_than_min_saved_rows() {
    let mut initial: BTreeMap<i64, StateGroupEntry> = BTreeMap::new();
    let mut prev = None;

    // This starts with the following structure
    //
    // 0-1-2 3-4-5 6-7-8 9-10-11 12-13
    //
    // Each group i has state:
    //     ('node','is',      i)
    //     ('group',  j, 'seen') - for all j less than i
    for i in 0i64..=13i64 {
        // if the state is a snapshot then set its predecessor to NONE
        if [0, 3, 6, 9, 12].contains(&i) {
            prev = None;
        }

        // create a blank entry for it
        let mut entry = StateGroupEntry {
            in_range: true,
            prev_state_group: prev,
            state_map: StateMap::new(),
        };

        // if it's a snapshot then add in all previous state
        if prev.is_none() {
            for j in 0i64..i {
                entry
                    .state_map
                    .insert("group", &j.to_string(), "seen".into());
            }
        }

        // add in the new state for this state group
        entry
            .state_map
            .insert("group", &i.to_string(), "seen".into());
        entry.state_map.insert("node", "is", i.to_string().into());

        // put it into the initial map
        initial.insert(i, entry);

        // set this group as the predecessor for the next
        prev = Some(i)
    }

    // Place this initial state into an empty database
    common::empty_database();
    common::add_contents_to_database("room1", &initial);

    // set up the config options
    let db_url = DB_URL.to_string();
    let output_file =
        "./tests/tmp/changes_not_commited_if_fewer_than_min_saved_rows.sql".to_string();
    let room_id = "room1".to_string();
    let min_state_group = "".to_string();
    let min_saved_rows = "12".to_string();
    let groups_to_compress = "".to_string();
    let level_sizes = "3,3".to_string();
    let transactions = true;
    let graphs = false;
    let commit_changes = true;

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
        commit_changes,
    );

    // Run the compressor with those settings
    run(config);

    // This should have created the following structure when running
    // (i.e. try and change groups 6 and 9 only)
    // BUT: This saves 11 rows which is fewer than min_saved_rows
    // therefore there should be no changes committed!
    //
    // 0  3\      12
    // 1  4 6\    13
    // 2  5 7 9
    //      8 10
    //        11

    // Check that the database still gives correct states for each group!
    assert!(database_collapsed_states_match_map(&initial));

    // Check that the structure of the database matches the expected structure
    assert!(database_structure_matches_map(&initial));
}

#[test]
#[should_panic]
fn run_panics_if_invalid_db_url() {
    // set up the config options
    let db_url = "thisIsAnInvalidURL".to_string();
    let output_file = "./tests/tmp/run_panics_if_invalid_db_url.sql".to_string();
    let room_id = "room1".to_string();
    let min_state_group = "".to_string();
    let min_saved_rows = "".to_string();
    let groups_to_compress = "".to_string();
    let level_sizes = "".to_string();
    let transactions = true;
    let graphs = false;
    let commit_changes = true;

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
        commit_changes,
    );

    // Run the compressor with those settings
    run(config);
}

#[test]
#[serial(db)]
fn run_only_affects_given_room_id() {
    // build room1 stuff up
    let mut initial_room_1: BTreeMap<i64, StateGroupEntry> = BTreeMap::new();
    let mut prev = None;

    // This starts with the following structure
    //
    // 0-1-2 3-4-5 6-7-8 9-10-11 12-13
    //
    // Each group i has state:
    //     ('node','is',      i)
    //     ('group',  j, 'seen') - for all j less than i
    for i in 0i64..=13i64 {
        // if the state is a snapshot then set its predecessor to NONE
        if [0, 3, 6, 9, 12].contains(&i) {
            prev = None;
        }

        // create a blank entry for it
        let mut entry = StateGroupEntry {
            in_range: true,
            prev_state_group: prev,
            state_map: StateMap::new(),
        };

        // if it's a snapshot then add in all previous state
        if prev.is_none() {
            for j in 0i64..i {
                entry
                    .state_map
                    .insert("group", &j.to_string(), "seen".into());
            }
        }

        // add in the new state for this state group
        entry
            .state_map
            .insert("group", &i.to_string(), "seen".into());
        entry.state_map.insert("node", "is", i.to_string().into());

        // put it into the initial map
        initial_room_1.insert(i, entry);

        // set this group as the predecessor for the next
        prev = Some(i)
    }

    // build room2 stuff up
    let mut initial_room_2: BTreeMap<i64, StateGroupEntry> = BTreeMap::new();
    let mut prev = None;

    // This starts with the same structure as room 1 but just all group ids
    // 14 higher
    for i in 14i64..=28i64 {
        // if the state is a snapshot then set its predecessor to NONE
        if [14, 17, 20, 23, 26].contains(&i) {
            prev = None;
        }

        // create a blank entry for it
        let mut entry = StateGroupEntry {
            in_range: true,
            prev_state_group: prev,
            state_map: StateMap::new(),
        };

        // if it's a snapshot then add in all previous state
        if prev.is_none() {
            for j in 14i64..i {
                entry
                    .state_map
                    .insert("group", &j.to_string(), "seen".into());
            }
        }

        // add in the new state for this state group
        entry
            .state_map
            .insert("group", &i.to_string(), "seen".into());
        entry.state_map.insert("node", "is", i.to_string().into());

        // put it into the initial map
        initial_room_2.insert(i, entry);

        // set this group as the predecessor for the next
        prev = Some(i)
    }

    // Place this initial state into an empty database
    common::empty_database();
    common::add_contents_to_database("room1", &initial_room_1);
    common::add_contents_to_database("room2", &initial_room_2);

    // set up the config options
    let db_url = DB_URL.to_string();
    let output_file = "./tests/tmp/run_only_affects_given_room_id.sql".to_string();
    let room_id = "room1".to_string();
    let min_state_group = "".to_string();
    let min_saved_rows = "".to_string();
    let groups_to_compress = "".to_string();
    let level_sizes = "3,3".to_string();
    let transactions = true;
    let graphs = false;
    let commit_changes = true;

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
        commit_changes,
    );

    // Run the compressor with those settings
    run(config);

    // This should have created the following structure in the database
    // i.e. groups 6 and 9 should have changed from before
    // N.B. this saves 11 rows
    //
    // 0  3\      12
    // 1  4 6\    13
    // 2  5 7 9
    //      8 10
    //        11
    let expected_edges: BTreeMap<i64, i64> = vec![
        (1, 0),
        (2, 1),
        (4, 3),
        (5, 4),
        (6, 3),
        (7, 6),
        (8, 7),
        (9, 6),
        (10, 9),
        (11, 10),
        (13, 12),
    ]
    .into_iter()
    .collect();

    let mut expected: BTreeMap<i64, StateGroupEntry> = BTreeMap::new();

    // Each group i has state:
    //     ('node','is',      i)
    //     ('group',  j, 'seen') - for all j less than i
    for i in 0i64..=13i64 {
        let prev = expected_edges.get(&i);

        //change from Option<&i64> to Option<i64>
        let prev = match prev {
            Some(p) => Some(*p),
            None => None,
        };

        // create a blank entry for it
        let mut entry = StateGroupEntry {
            in_range: true,
            prev_state_group: prev,
            state_map: StateMap::new(),
        };

        // Add in all state between predecessor and now (non inclusive)
        if let Some(p) = prev {
            for j in (p + 1)..i {
                entry
                    .state_map
                    .insert("group", &j.to_string(), "seen".into());
            }
        } else {
            for j in 0i64..i {
                entry
                    .state_map
                    .insert("group", &j.to_string(), "seen".into());
            }
        }

        // add in the new state for this state group
        entry
            .state_map
            .insert("group", &i.to_string(), "seen".into());
        entry.state_map.insert("node", "is", i.to_string().into());

        // put it into the expected map
        expected.insert(i, entry);
    }

    // Check that the database still gives correct states for each group
    // in both room1 and room2
    assert!(database_collapsed_states_match_map(&initial_room_1));
    assert!(database_collapsed_states_match_map(&initial_room_2));

    // Check that the structure of the database matches the expected structure
    // in both room1 and room2
    assert!(database_structure_matches_map(&expected));
    assert!(database_structure_matches_map(&initial_room_2));
}

#[test]
#[serial(db)]
fn run_respects_min_state_group() {
    let mut initial: BTreeMap<i64, StateGroupEntry> = BTreeMap::new();
    let mut prev = None;

    // This starts with the following structure
    //
    // 0-1-2 3-4-5 6-7-8 9-10-11 12-13
    //
    // Each group i has state:
    //     ('node','is',      i)
    //     ('group',  j, 'seen') - for all j less than i
    for i in 0i64..=13i64 {
        // if the state is a snapshot then set its predecessor to NONE
        if [0, 3, 6, 9, 12].contains(&i) {
            prev = None;
        }

        // create a blank entry for it
        let mut entry = StateGroupEntry {
            in_range: true,
            prev_state_group: prev,
            state_map: StateMap::new(),
        };

        // if it's a snapshot then add in all previous state
        if prev.is_none() {
            for j in 0i64..i {
                entry
                    .state_map
                    .insert("group", &j.to_string(), "seen".into());
            }
        }

        // add in the new state for this state group
        entry
            .state_map
            .insert("group", &i.to_string(), "seen".into());
        entry.state_map.insert("node", "is", i.to_string().into());

        // put it into the initial map
        initial.insert(i, entry);

        // set this group as the predecessor for the next
        prev = Some(i)
    }

    // Place this initial state into an empty database
    common::empty_database();
    common::add_contents_to_database("room1", &initial);

    // set up the config options
    let db_url = DB_URL.to_string();
    let output_file = "./tests/tmp/run_respects_min_state_group.sql".to_string();
    let room_id = "room1".to_string();
    let min_state_group = "2".to_string();
    let min_saved_rows = "".to_string();
    let groups_to_compress = "".to_string();
    let level_sizes = "3,3".to_string();
    let transactions = true;
    let graphs = false;
    let commit_changes = true;

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
        commit_changes,
    );

    // Run the compressor with those settings
    run(config);

    // This should have created the following structure in the database
    // as it should only compress from groups higher than 2 (non inclusive)
    // i.e. groups 9 and 12 should have changed from before
    // N.B. this saves 17 rows
    //
    // 0  3  6\
    // 1  4  7  9\
    // 2  5  8 10 12
    //         11 13
    //
    let expected_edges: BTreeMap<i64, i64> = vec![
        (1, 0),
        (2, 1),
        (4, 3),
        (5, 4),
        (7, 6),
        (8, 7),
        (9, 6),
        (10, 9),
        (11, 10),
        (12, 9),
        (13, 12),
    ]
    .into_iter()
    .collect();

    let mut expected: BTreeMap<i64, StateGroupEntry> = BTreeMap::new();

    // Each group i has state:
    //     ('node','is',      i)
    //     ('group',  j, 'seen') - for all j less than i
    for i in 0i64..=13i64 {
        let prev = expected_edges.get(&i);

        //change from Option<&i64> to Option<i64>
        let prev = match prev {
            Some(p) => Some(*p),
            None => None,
        };

        // create a blank entry for it
        let mut entry = StateGroupEntry {
            in_range: true,
            prev_state_group: prev,
            state_map: StateMap::new(),
        };

        // Add in all state between predecessor and now (non inclusive)
        if let Some(p) = prev {
            for j in (p + 1)..i {
                entry
                    .state_map
                    .insert("group", &j.to_string(), "seen".into());
            }
        } else {
            for j in 0i64..i {
                entry
                    .state_map
                    .insert("group", &j.to_string(), "seen".into());
            }
        }

        // add in the new state for this state group
        entry
            .state_map
            .insert("group", &i.to_string(), "seen".into());
        entry.state_map.insert("node", "is", i.to_string().into());

        // put it into the expected map
        expected.insert(i, entry);
    }

    // Check that the database still gives correct states for each group!
    assert!(database_collapsed_states_match_map(&initial));

    // Check that the structure of the database matches the expected structure
    assert!(database_structure_matches_map(&expected))
}

#[test]
#[serial(db)]
fn run_respects_groups_to_compress() {
    let mut initial: BTreeMap<i64, StateGroupEntry> = BTreeMap::new();
    let mut prev = None;

    // This starts with the following structure
    //
    // 0-1-2 3-4-5 6-7-8 9-10-11 12-13
    //
    // Each group i has state:
    //     ('node','is',      i)
    //     ('group',  j, 'seen') - for all j less than i
    for i in 0i64..=13i64 {
        // if the state is a snapshot then set its predecessor to NONE
        if [0, 3, 6, 9, 12].contains(&i) {
            prev = None;
        }

        // create a blank entry for it
        let mut entry = StateGroupEntry {
            in_range: true,
            prev_state_group: prev,
            state_map: StateMap::new(),
        };

        // if it's a snapshot then add in all previous state
        if prev.is_none() {
            for j in 0i64..i {
                entry
                    .state_map
                    .insert("group", &j.to_string(), "seen".into());
            }
        }

        // add in the new state for this state group
        entry
            .state_map
            .insert("group", &i.to_string(), "seen".into());
        entry.state_map.insert("node", "is", i.to_string().into());

        // put it into the initial map
        initial.insert(i, entry);

        // set this group as the predecessor for the next
        prev = Some(i)
    }

    // Place this initial state into an empty database
    common::empty_database();
    common::add_contents_to_database("room1", &initial);

    // set up the config options
    let db_url = DB_URL.to_string();
    let output_file = "./tests/tmp/run_respects_groups_to_compress.sql".to_string();
    let room_id = "room1".to_string();
    let min_state_group = "2".to_string();
    let min_saved_rows = "".to_string();
    let groups_to_compress = "9".to_string();
    let level_sizes = "3,3".to_string();
    let transactions = true;
    let graphs = false;
    let commit_changes = true;

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
        commit_changes,
    );

    // Run the compressor with those settings
    run(config);

    // This should have created the following structure in the database
    // as it should only compress from groups higher than 2 (non inclusive)
    // and should only compress a total of 9 groups
    // i.e. so only group 9 should have changed from before
    // N.B. this saves 7 rows
    //
    // 0  3  6\    12
    // 1  4  7  9  13
    // 2  5  8 10
    //         11
    //
    let expected_edges: BTreeMap<i64, i64> = vec![
        (1, 0),
        (2, 1),
        (4, 3),
        (5, 4),
        (7, 6),
        (8, 7),
        (9, 6),
        (10, 9),
        (11, 10),
        (13, 12),
    ]
    .into_iter()
    .collect();

    let mut expected: BTreeMap<i64, StateGroupEntry> = BTreeMap::new();

    // Each group i has state:
    //     ('node','is',      i)
    //     ('group',  j, 'seen') - for all j less than i
    for i in 0i64..=13i64 {
        let prev = expected_edges.get(&i);

        //change from Option<&i64> to Option<i64>
        let prev = match prev {
            Some(p) => Some(*p),
            None => None,
        };

        // create a blank entry for it
        let mut entry = StateGroupEntry {
            in_range: true,
            prev_state_group: prev,
            state_map: StateMap::new(),
        };

        // Add in all state between predecessor and now (non inclusive)
        if let Some(p) = prev {
            for j in (p + 1)..i {
                entry
                    .state_map
                    .insert("group", &j.to_string(), "seen".into());
            }
        } else {
            for j in 0i64..i {
                entry
                    .state_map
                    .insert("group", &j.to_string(), "seen".into());
            }
        }

        // add in the new state for this state group
        entry
            .state_map
            .insert("group", &i.to_string(), "seen".into());
        entry.state_map.insert("node", "is", i.to_string().into());

        // put it into the expected map
        expected.insert(i, entry);
    }

    // Check that the database still gives correct states for each group!
    assert!(database_collapsed_states_match_map(&initial));

    // Check that the structure of the database matches the expected structure
    assert!(database_structure_matches_map(&expected))
}
