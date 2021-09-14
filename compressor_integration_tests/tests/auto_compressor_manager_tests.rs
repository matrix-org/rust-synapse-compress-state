use std::collections::BTreeMap;

use auto_compressor::{
    manager::{compress_largest_rooms, run_compressor_on_room_chunk},
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
use state_map::StateMap;
use synapse_compress_state::{Level, StateGroupEntry};

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
fn compress_largest_rooms_compresses_multiple_rooms() {
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

    // compress the largest 10 rooms in chunks of size 7
    // (Note only 2 rooms should exist in the database, but this should not panic)
    compress_largest_rooms(DB_URL, 7, &default_levels, 10).unwrap();

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
fn compress_largest_rooms_does_largest_rooms() {
    setup_logger();
    // This creates 2 with the following structure
    //
    // 0-1-2 3-4-5 (room1)
    // 14-15-16 17-18-19 20-21-22 23-24-25 26-27 (room2)
    //
    // Each group i has state:
    //     ('node','is',      i)
    //     ('group',  j, 'seen') - for all j less than i in that room

    // NOTE the second room has more state

    let initial1 = line_segments_with_state(0, 5);
    let initial2 = line_segments_with_state(14, 27);

    empty_database();
    add_contents_to_database("room1", &initial1);
    add_contents_to_database("room2", &initial2);

    let mut client = connect_to_database(DB_URL).unwrap();
    create_tables_if_needed(&mut client).unwrap();
    clear_compressor_state();

    // compress in 3,3 level sizes by default
    let default_levels = vec![Level::new(3), Level::new(3)];

    // compress the largest 1 rooms in chunks of size 7
    // (Note this should ONLY compress room2 since it has more state)
    compress_largest_rooms(DB_URL, 7, &default_levels, 1).unwrap();

    // We are aiming for the following structure in the database for room2
    // i.e. groups 20 and 23 should have changed from initial map
    // N.B. this saves 11 rows
    //
    // 14  17\       26
    // 15  18 20\    27
    // 16  19 21 23
    //        22 24
    //           25
    //
    // Where each group i has state:
    //     ('node','is',      i)
    //     ('group',  j, 'seen') - for all j less than i in that room
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

    // Check that the database still gives correct states for each group in room1
    assert!(database_collapsed_states_match_map(&initial1));

    // Check that the structure of the database is still what it was initially
    assert!(database_structure_matches_map(&initial1));

    // Check that the database still gives correct states for each group in room2
    assert!(database_collapsed_states_match_map(&initial2));

    // Check that the structure of the database is the expected compressed one
    assert!(database_structure_matches_map(&expected2));
}

#[test]
#[serial(db)]
fn compress_largest_rooms_skips_already_compressed_when_rerun() {
    setup_logger();
    // This test builds two rooms in the database and then calls compress_largest_rooms
    // with a number argument of 1 (i.e. only the larger of the two rooms should be
    // compressed)
    //
    // It then adds another state group to the larger of the two rooms and calls the
    // compress_largest_rooms function again. However there is more UNCOMPRESSED state
    // in the smaller room, so the new state added to the larger room should remain
    // untouched
    //
    // This is meant to simulate events happening in rooms between calls to the function

    // Initially create 2 rooms with the following structure
    //
    // 0-1-2 3-4-5 6-7-8 9-10-11 12-13(room1)
    // 14-15-16 17-18-19 20 (room2)
    //
    // Each group i has state:
    //     ('node','is',      i)
    //     ('group',  j, 'seen') - for all j less than i in that room
    // NOTE the first room is the larger one
    let initial1 = line_segments_with_state(0, 13);
    let initial2 = line_segments_with_state(14, 20);

    empty_database();
    add_contents_to_database("room1", &initial1);
    add_contents_to_database("room2", &initial2);

    let mut client = connect_to_database(DB_URL).unwrap();
    create_tables_if_needed(&mut client).unwrap();
    clear_compressor_state();

    // compress in 3,3 level sizes by default
    let default_levels = vec![Level::restore(3, 0, None), Level::restore(3, 0, None)];

    // compress the largest 1 rooms in chunks of size 7
    // (Note this should ONLY compress room1 since it has more state)
    compress_largest_rooms(DB_URL, 7, &default_levels, 1).unwrap();

    // This should have created the following structure in the database
    // i.e. groups 6 and 9 should have changed from before
    // N.B. this saves 11 rows
    //
    // 0  3\      12
    // 1  4 6\    13
    // 2  5 7 9
    //      8 10
    //        11
    // room 2 should be unchanged
    let expected1 = compressed_3_3_from_0_to_13_with_state();

    // room1 is new structure, room2 is as it was initially
    assert!(database_structure_matches_map(&expected1));
    assert!(database_structure_matches_map(&initial2));

    // Now add another state group to room1 with predecessor 12
    //
    // If the compressor is run on room1 again then the prev_state_group for 21
    // will be set to 13
    //
    // i.e the compressor would try and build the following:
    //
    // 0  3\      12
    // 1  4 6\    13
    // 2  5 7 9   21
    //      8 10
    //        11

    let mut initial1_with_new_group = expected1.clone();

    let mut group21 = StateGroupEntry {
        in_range: true,
        prev_state_group: Some(12),
        state_map: StateMap::new(),
    };

    // add in the new state for this state group
    group21.state_map.insert("group", "13", "seen".into());
    group21.state_map.insert("group", "21", "seen".into());
    group21.state_map.insert("node", "is", "21".into());

    initial1_with_new_group.insert(21, group21);

    // Actually send this group to the database
    let sql = r#"
        INSERT INTO state_groups (id, room_id, event_id) VALUES (21,'room1','left_blank');
        INSERT INTO state_group_edges (state_group, prev_state_group) VALUES 
            (21,12);
        INSERT INTO state_groups_state (state_group, room_id, type, state_key, event_id) VALUES
            (21,'room1','node', 'is', '21'),
            (21,'room1','group', '13', 'seen'),
            (21,'room1','group', '21', 'seen');
    "#;
    client.batch_execute(sql).unwrap();

    // We are aiming for the following structure in the database for room2
    // i.e. only group 20 hould have changed from initial map
    //
    // 14  17\
    // 15  18 20
    // 16  19
    //
    // Where each group i has state:
    //     ('node','is',      i)
    //     ('group',  j, 'seen') - for all j less than i in that room
    let expected_edges: BTreeMap<i64, i64> = vec![(15, 14), (16, 15), (18, 17), (19, 18), (20, 17)]
        .into_iter()
        .collect();

    let expected2 = structure_from_edges_with_state(expected_edges, 14, 20);

    // compress the largest 1 rooms in chunks of size 7
    // (Note this should ONLY compress room2 since room1 only has 1 uncompressed state group)
    compress_largest_rooms(DB_URL, 7, &default_levels, 1).unwrap();

    // Check that the database still gives correct states for each group in room1
    assert!(database_collapsed_states_match_map(
        &initial1_with_new_group
    ));

    // Check that the structure of the database is still what it was befeore
    // compress_largest_rooms was called (i.e. that pred of 21 is still 12 not now 13)
    assert!(database_structure_matches_map(&initial1_with_new_group));

    // Check that the database still gives correct states for each group in room2
    assert!(database_collapsed_states_match_map(&initial2));
    // Check that the structure of the database is the expected compressed one
    assert!(database_structure_matches_map(&expected2));
}
