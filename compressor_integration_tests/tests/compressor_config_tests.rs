use std::collections::BTreeMap;

use compressor_integration_tests::{
    add_contents_to_database, database_collapsed_states_match_map, database_structure_matches_map,
    empty_database,
    map_builder::{
        compressed_3_3_from_0_to_13_with_state, line_segments_with_state, line_with_state,
        structure_from_edges_with_state,
    },
    DB_URL,
};
use serial_test::serial;
use synapse_compress_state::{run, Config};

// Remember to add #[serial(db)] before any test that access the database.
// Only one test with this annotation can run at once - preventing
// concurrency bugs.
//
// You will probably also want to use common::empty_database() at the start
// of each test as well (since their order of execution is not guaranteed)

#[test]
#[serial(db)]
fn run_succeeds_without_crashing() {
    // This starts with the following structure
    //
    // 0-1-2-3-4-5-6-7-8-9-10-11-12-13
    //
    // Each group i has state:
    //     ('node','is',      i)
    //     ('group',  j, 'seen') - for all j less than i
    let initial = line_with_state(0, 13);

    empty_database();
    add_contents_to_database("room1", &initial);

    let db_url = DB_URL.to_string();
    let room_id = "room1".to_string();
    let output_file = Some("./tests/tmp/run_succeeds_without_crashing.sql".to_string());
    let min_state_group = None;
    let groups_to_compress = None;
    let min_saved_rows = None;
    let max_state_group = None;
    let level_sizes = "3,3".to_string();
    let transactions = true;
    let graphs = false;
    let commit_changes = false;

    let config = Config::new(
        db_url.clone(),
        room_id.clone(),
        output_file,
        min_state_group,
        groups_to_compress,
        min_saved_rows,
        max_state_group,
        level_sizes,
        transactions,
        graphs,
        commit_changes,
    )
    .unwrap();

    run(config);
}

#[test]
#[serial(db)]
fn changes_commited_if_no_min_saved_rows() {
    // This starts with the following structure
    //
    // 0-1-2 3-4-5 6-7-8 9-10-11 12-13
    //
    // Each group i has state:
    //     ('node','is',      i)
    //     ('group',  j, 'seen') - for all j less than i
    let initial = line_segments_with_state(0, 13);

    // Place this initial state into an empty database
    empty_database();
    add_contents_to_database("room1", &initial);

    // set up the config options
    let db_url = DB_URL.to_string();
    let room_id = "room1".to_string();
    let output_file = Some("./tests/tmp/changes_commited_if_no_min_saved_rows.sql".to_string());
    let min_state_group = None;
    let min_saved_rows = None;
    let groups_to_compress = None;
    let max_state_group = None;
    let level_sizes = "3,3".to_string();
    let transactions = true;
    let graphs = false;
    let commit_changes = true;

    let config = Config::new(
        db_url,
        room_id,
        output_file,
        min_state_group,
        groups_to_compress,
        min_saved_rows,
        max_state_group,
        level_sizes,
        transactions,
        graphs,
        commit_changes,
    )
    .unwrap();

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
    let expected = compressed_3_3_from_0_to_13_with_state();

    // Check that the database still gives correct states for each group!
    assert!(database_collapsed_states_match_map(&initial));

    // Check that the structure of the database matches the expected structure
    assert!(database_structure_matches_map(&expected))
}

#[test]
#[serial(db)]
fn changes_commited_if_min_saved_rows_exceeded() {
    // This starts with the following structure
    //
    // 0-1-2 3-4-5 6-7-8 9-10-11 12-13
    //
    // Each group i has state:
    //     ('node','is',      i)
    //     ('group',  j, 'seen') - for all j less than i
    let initial = line_segments_with_state(0, 13);

    // Place this initial state into an empty database
    empty_database();
    add_contents_to_database("room1", &initial);

    // set up the config options
    let db_url = DB_URL.to_string();
    let room_id = "room1".to_string();
    let output_file = Some("./tests/tmp/changes_commited_if_no_min_saved_rows.sql".to_string());
    let min_state_group = None;
    let min_saved_rows = Some(10);
    let groups_to_compress = None;
    let max_state_group = None;
    let level_sizes = "3,3".to_string();
    let transactions = true;
    let graphs = false;
    let commit_changes = true;

    let config = Config::new(
        db_url,
        room_id,
        output_file,
        min_state_group,
        groups_to_compress,
        min_saved_rows,
        max_state_group,
        level_sizes,
        transactions,
        graphs,
        commit_changes,
    )
    .unwrap();

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
    let expected = compressed_3_3_from_0_to_13_with_state();

    // Check that the database still gives correct states for each group!
    assert!(database_collapsed_states_match_map(&initial));

    // Check that the structure of the database matches the expected structure
    assert!(database_structure_matches_map(&expected));
}

#[test]
#[serial(db)]
fn changes_not_commited_if_fewer_than_min_saved_rows() {
    // This starts with the following structure
    //
    // 0-1-2 3-4-5 6-7-8 9-10-11 12-13
    //
    // Each group i has state:
    //     ('node','is',      i)
    //     ('group',  j, 'seen') - for all j less than i
    let initial = line_segments_with_state(0, 13);

    // Place this initial state into an empty database
    empty_database();
    add_contents_to_database("room1", &initial);

    // set up the config options
    let db_url = DB_URL.to_string();
    let room_id = "room1".to_string();
    let output_file =
        Some("./tests/tmp/changes_not_commited_if_fewer_than_min_saved_rows.sql".to_string());
    let min_state_group = None;
    let min_saved_rows = Some(12);
    let groups_to_compress = None;
    let max_state_group = None;
    let level_sizes = "3,3".to_string();
    let transactions = true;
    let graphs = false;
    let commit_changes = true;

    let config = Config::new(
        db_url,
        room_id,
        output_file,
        min_state_group,
        groups_to_compress,
        min_saved_rows,
        max_state_group,
        level_sizes,
        transactions,
        graphs,
        commit_changes,
    )
    .unwrap();

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
#[should_panic(expected = "Error connecting to the database:")]
fn run_panics_if_invalid_db_url() {
    // set up the config options
    let db_url = "thisIsAnInvalidURL".to_string();
    let room_id = "room1".to_string();
    let output_file = Some("./tests/tmp/run_panics_if_invalid_db_url.sql".to_string());
    let min_state_group = None;
    let min_saved_rows = None;
    let groups_to_compress = None;
    let max_state_group = None;
    let level_sizes = "3,3".to_string();
    let transactions = true;
    let graphs = false;
    let commit_changes = true;

    let config = Config::new(
        db_url,
        room_id,
        output_file,
        min_state_group,
        groups_to_compress,
        min_saved_rows,
        max_state_group,
        level_sizes,
        transactions,
        graphs,
        commit_changes,
    )
    .unwrap();

    // Run the compressor with those settings
    run(config);
}

#[test]
#[serial(db)]
fn run_only_affects_given_room_id() {
    // build room1 stuff up
    // This starts with the following structure
    //
    // 0-1-2 3-4-5 6-7-8 9-10-11 12-13
    //
    // Each group i has state:
    //     ('node','is',      i)
    //     ('group',  j, 'seen') - for all j less than i
    let initial_room_1 = line_segments_with_state(0, 13);

    // build room2 stuff up
    // This starts with the same structure as room 1 but just all group ids
    // 14 higher
    let initial_room_2 = line_segments_with_state(14, 28);

    // Place this initial state into an empty database
    empty_database();
    add_contents_to_database("room1", &initial_room_1);
    add_contents_to_database("room2", &initial_room_2);

    // set up the config options
    let db_url = DB_URL.to_string();
    let room_id = "room1".to_string();
    let output_file = Some("./tests/tmp/run_only_affects_given_room_id.sql".to_string());
    let min_state_group = None;
    let min_saved_rows = None;
    let groups_to_compress = None;
    let max_state_group = None;
    let level_sizes = "3,3".to_string();
    let transactions = true;
    let graphs = false;
    let commit_changes = true;

    let config = Config::new(
        db_url,
        room_id,
        output_file,
        min_state_group,
        groups_to_compress,
        min_saved_rows,
        max_state_group,
        level_sizes,
        transactions,
        graphs,
        commit_changes,
    )
    .unwrap();

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
    let expected = compressed_3_3_from_0_to_13_with_state();

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
fn run_respects_groups_to_compress() {
    // This starts with the following structure
    //
    // 0-1-2 3-4-5 6-7-8 9-10-11 12-13
    //
    // Each group i has state:
    //     ('node','is',      i)
    //     ('group',  j, 'seen') - for all j less than i
    let initial = line_segments_with_state(0, 13);

    // Place this initial state into an empty database
    empty_database();
    add_contents_to_database("room1", &initial);

    // set up the config options
    let db_url = DB_URL.to_string();
    let room_id = "room1".to_string();
    let output_file = Some("./tests/tmp/run_respects_groups_to_compress.sql".to_string());
    let min_state_group = Some(2);
    let min_saved_rows = None;
    let groups_to_compress = Some(9);
    let max_state_group = None;
    let level_sizes = "3,3".to_string();
    let transactions = true;
    let graphs = false;
    let commit_changes = true;

    let config = Config::new(
        db_url,
        room_id,
        output_file,
        min_state_group,
        groups_to_compress,
        min_saved_rows,
        max_state_group,
        level_sizes,
        transactions,
        graphs,
        commit_changes,
    )
    .unwrap();

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

    let expected = structure_from_edges_with_state(expected_edges, 0, 13);

    // Check that the database still gives correct states for each group!
    assert!(database_collapsed_states_match_map(&initial));

    // Check that the structure of the database matches the expected structure
    assert!(database_structure_matches_map(&expected))
}

#[test]
#[serial(db)]
fn run_is_idempotent_when_run_on_whole_room() {
    // This starts with the following structure
    //
    // 0-1-2 3-4-5 6-7-8 9-10-11 12-13
    //
    // Each group i has state:
    //     ('node','is',      i)
    //     ('group',  j, 'seen') - for all j less than i
    let initial = line_segments_with_state(0, 13);

    // Place this initial state into an empty database
    empty_database();
    add_contents_to_database("room1", &initial);

    // set up the config options
    let db_url = DB_URL.to_string();
    let room_id = "room1".to_string();
    let output_file1 =
        Some("./tests/tmp/run_is_idempotent_when_run_on_whole_room_1.sql".to_string());
    let output_file2 =
        Some("./tests/tmp/run_is_idempotent_when_run_on_whole_room_2.sql".to_string());
    let min_state_group = None;
    let min_saved_rows = None;
    let groups_to_compress = None;
    let max_state_group = None;
    let level_sizes = "3,3".to_string();
    let transactions = true;
    let graphs = false;
    let commit_changes = true;

    let config1 = Config::new(
        db_url.clone(),
        room_id.clone(),
        output_file1,
        min_state_group,
        groups_to_compress,
        min_saved_rows,
        max_state_group,
        level_sizes.clone(),
        transactions,
        graphs,
        commit_changes,
    )
    .unwrap();

    let config2 = Config::new(
        db_url.clone(),
        room_id.clone(),
        output_file2,
        min_state_group,
        groups_to_compress,
        min_saved_rows,
        max_state_group,
        level_sizes.clone(),
        transactions,
        graphs,
        commit_changes,
    )
    .unwrap();

    // We are aiming for the following structure in the database
    // i.e. groups 6 and 9 should have changed from initial map
    // N.B. this saves 11 rows
    //
    // 0  3\      12
    // 1  4 6\    13
    // 2  5 7 9
    //      8 10
    //        11
    //
    // Where each group i has state:
    //     ('node','is',      i)
    //     ('group',  j, 'seen') - for all j less than i
    let expected = compressed_3_3_from_0_to_13_with_state();

    // Run the compressor with those settings for the first time
    run(config1);

    // Check that the database still gives correct states for each group!
    assert!(database_collapsed_states_match_map(&initial));

    // Check that the structure of the database matches the expected structure
    assert!(database_structure_matches_map(&expected));

    // Run the compressor with those settings for the second time
    run(config2);

    // Check that the database still gives correct states for each group!
    assert!(database_collapsed_states_match_map(&initial));

    // Check that the structure of the database still matches the expected structure
    assert!(database_structure_matches_map(&expected));
}
