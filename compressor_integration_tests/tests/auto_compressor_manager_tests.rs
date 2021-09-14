use auto_compressor::{
    manager::{run_compressor_on_room_chunk},
    state_saving::{connect_to_database, create_tables_if_needed},
};
use compressor_integration_tests::{
    add_contents_to_database, clear_compressor_state, database_collapsed_states_match_map,
    database_structure_matches_map, empty_database,
    map_builder::{
        compressed_3_3_from_0_to_13_with_state, line_segments_with_state,
    },
    setup_logger, DB_URL,
};
use serial_test::serial;
use synapse_compress_state::{Level};

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
    let default_levels = vec![Level::restore(3, 0, None), Level::restore(3, 0, None)];

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
