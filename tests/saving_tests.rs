use std::collections::BTreeMap;

use serial_test::serial;
use state_map::StateMap;
use synapse_compress_state::{continue_run, StateGroupEntry};

mod common;

// Tests the saving and continuing functionality
// The compressor should produce the same results when run in one go
// as when run in multiple stages
#[test]
#[serial(db)]
fn continue_run_called_twice_same_as_run() {
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

    let db_url = common::DB_URL.to_string();
    let room_id = "room1".to_string();

    // will run the compression in two batches
    let start = -1; // 1st group id here is 0 so set start to -1
    let chunk_size = 7;
    // compress in 3,3 level sizes
    // since the compressor hasn't been run before they are emtpy
    let level_info = vec![(3, 0, None), (3, 0, None)];

    // Run the compressor with those settings
    let chunk_stats_1 = continue_run(start, chunk_size, &db_url, &room_id, &level_info);
    // Assert that it stopped at 6 (i.e. after the 7 groups 0...6)
    assert_eq!(chunk_stats_1.last_compressed_group, 6);
    // structure should be the following at this poing
    // (NOTE: only including compressed groups)
    //
    // 0  3\
    // 1  4 6
    // 2  5
    assert_eq!(
        chunk_stats_1.new_level_info,
        vec![(3, 1, Some(6)), (3, 2, Some(6))]
    );

    let start = 6;
    let chunk_size = 7;
    let level_info = chunk_stats_1.new_level_info.clone();

    // Run the compressor with those settings
    let chunk_stats_2 = continue_run(start, chunk_size, &db_url, &room_id, &level_info);
    // Assert that it stopped at 7
    assert_eq!(chunk_stats_2.last_compressed_group, 13);

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
    assert!(common::database_collapsed_states_match_map(&initial));

    // Check that the structure of the database matches the expected structure
    assert!(common::database_structure_matches_map(&expected))
}
