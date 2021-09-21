use std::collections::BTreeMap;

use auto_compressor::{
    manager::{compress_chunks_of_database, run_compressor_on_room_chunk},
    state_saving::{connect_to_database, create_tables_if_needed},
};
use compressor_integration_tests::{
    add_contents_to_database, clear_compressor_state, database_collapsed_states_match_map,
    database_structure_matches_map, empty_database,
    map_builder::{
        compressed_3_3_from_0_to_13_with_state, line_segments_with_state,
        structure_from_edges_with_state,
    },
    setup_logger, DB_URL,
};
use serial_test::serial;
use synapse_compress_state::Level;

#[test]
#[serial(db)]
fn run_compressor_on_room_chunk_works() {
    setup_logger();
    // This starts with the following structure
    //
    // 0-1-2 3-4-5 6-7-8 9-10-11 12-13
    //
    // Each group i has state:
    //     ('node','is',      i)
    //     ('group',  j, 'seen') - for all j less than i
    let initial = line_segments_with_state(0, 13);
    empty_database();
    add_contents_to_database("room1", &initial);

    let mut client = connect_to_database(DB_URL).unwrap();
    create_tables_if_needed(&mut client).unwrap();
    clear_compressor_state();

    // compress in 3,3 level sizes by default
    let default_levels = vec![Level::new(3), Level::new(3)];

    // compress the first 7 groups in the room
    // structure should be the following afterwards
    // (NOTE: only including compressed groups)
    //
    // 0  3\
    // 1  4 6
    // 2  5
    run_compressor_on_room_chunk(DB_URL, "room1", 7, &default_levels).unwrap();

    // compress the next 7 groups

    run_compressor_on_room_chunk(DB_URL, "room1", 7, &default_levels).unwrap();

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
fn compress_chunks_of_database_compresses_multiple_rooms() {
    setup_logger();
    // This creates 2 with the following structure
    //
    // 0-1-2 3-4-5 6-7-8 9-10-11 12-13
    // (with room2's numbers shifted up 14)
    //
    // Each group i has state:
    //     ('node','is',      i)
    //     ('group',  j, 'seen') - for all j less than i in that room
    let initial1 = line_segments_with_state(0, 13);
    let initial2 = line_segments_with_state(14, 27);

    empty_database();
    add_contents_to_database("room1", &initial1);
    add_contents_to_database("room2", &initial2);

    let mut client = connect_to_database(DB_URL).unwrap();
    create_tables_if_needed(&mut client).unwrap();
    clear_compressor_state();

    // compress in 3,3 level sizes by default
    let default_levels = vec![Level::new(3), Level::new(3)];

    // Compress 4 chunks of size 8.
    // The first two should compress room1 and the second two should compress room2
    compress_chunks_of_database(DB_URL, 8, &default_levels, 4).unwrap();

    // We are aiming for the following structure in the database for room1
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
    let expected1 = compressed_3_3_from_0_to_13_with_state();

    // Check that the database still gives correct states for each group in room1
    assert!(database_collapsed_states_match_map(&initial1));

    // Check that the structure of the database matches the expected structure for room1
    assert!(database_structure_matches_map(&expected1));

    // room 2 should have the same structure but will all numbers shifted up by 14
    let expected_edges: BTreeMap<i64, i64> = vec![
        (15, 14),
        (16, 15),
        (18, 17),
        (19, 18),
        (20, 17),
        (21, 20),
        (22, 21),
        (23, 20),
        (24, 23),
        (25, 24),
        (27, 26),
    ]
    .into_iter()
    .collect();

    let expected2 = structure_from_edges_with_state(expected_edges, 14, 27);

    // Check that the database still gives correct states for each group in room2
    assert!(database_collapsed_states_match_map(&initial2));

    // Check that the structure of the database matches the expected structure for room2
    assert!(database_structure_matches_map(&expected2));
}

#[test]
#[serial(db)]
fn compress_chunks_of_database_continues_where_it_left_off() {
    setup_logger();
    // This creates 2 with the following structure
    //
    // 0-1-2 3-4-5 6-7-8 9-10-11 12-13
    // (with room2's numbers shifted up 14)
    //
    // Each group i has state:
    //     ('node','is',      i)
    //     ('group',  j, 'seen') - for all j less than i in that room
    let initial1 = line_segments_with_state(0, 13);
    let initial2 = line_segments_with_state(14, 27);

    empty_database();
    add_contents_to_database("room1", &initial1);
    add_contents_to_database("room2", &initial2);

    let mut client = connect_to_database(DB_URL).unwrap();
    create_tables_if_needed(&mut client).unwrap();
    clear_compressor_state();

    // compress in 3,3 level sizes by default
    let default_levels = vec![Level::new(3), Level::new(3)];

    // Compress chunks of various sizes:
    //
    // These two should compress room1
    compress_chunks_of_database(DB_URL, 8, &default_levels, 1).unwrap();
    compress_chunks_of_database(DB_URL, 100, &default_levels, 1).unwrap();
    // These three should compress room2
    compress_chunks_of_database(DB_URL, 1, &default_levels, 2).unwrap();
    compress_chunks_of_database(DB_URL, 5, &default_levels, 1).unwrap();
    compress_chunks_of_database(DB_URL, 5, &default_levels, 1).unwrap();

    // We are aiming for the following structure in the database for room1
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
    let expected1 = compressed_3_3_from_0_to_13_with_state();

    // Check that the database still gives correct states for each group in room1
    assert!(database_collapsed_states_match_map(&initial1));

    // Check that the structure of the database matches the expected structure for room1
    assert!(database_structure_matches_map(&expected1));

    // room 2 should have the same structure but will all numbers shifted up by 14
    let expected_edges: BTreeMap<i64, i64> = vec![
        (15, 14),
        (16, 15),
        (18, 17),
        (19, 18),
        (20, 17),
        (21, 20),
        (22, 21),
        (23, 20),
        (24, 23),
        (25, 24),
        (27, 26),
    ]
    .into_iter()
    .collect();

    let expected2 = structure_from_edges_with_state(expected_edges, 14, 27);

    // Check that the database still gives correct states for each group in room2
    assert!(database_collapsed_states_match_map(&initial2));

    // Check that the structure of the database matches the expected structure for room2
    assert!(database_structure_matches_map(&expected2));
}
