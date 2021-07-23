use compressor_integration_tests::{
    add_contents_to_database, database_collapsed_states_match_map, database_structure_matches_map,
    empty_database,
    map_builder::{compressed_3_3_from_0_to_13_with_state, line_segments_with_state},
    DB_URL,
};
use serial_test::serial;
use synapse_compress_state::continue_run;

// Tests the saving and continuing functionality
// The compressor should produce the same results when run in one go
// as when run in multiple stages
#[test]
#[serial(db)]
fn continue_run_called_twice_same_as_run() {
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

    let db_url = DB_URL.to_string();
    let room_id = "room1".to_string();

    // will run the compression in two batches
    let start = -1;
    let chunk_size = 7;

    // compress in 3,3 level sizes
    // since the compressor hasn't been run before they are empty
    let level_info = vec![(3, 0, None), (3, 0, None)];

    // Run the compressor with those settings
    let chunk_stats_1 = continue_run(start, chunk_size, &db_url, &room_id, &level_info);

    // Assert that it stopped at 6 (i.e. after the 7 groups 0...6)
    assert_eq!(chunk_stats_1.last_compressed_group, 6);
    // structure should be the following at this point
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
    let expected = compressed_3_3_from_0_to_13_with_state();

    // Check that the database still gives correct states for each group!
    assert!(database_collapsed_states_match_map(&initial));

    // Check that the structure of the database matches the expected structure
    assert!(database_structure_matches_map(&expected))
}
