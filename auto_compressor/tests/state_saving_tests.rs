use auto_compressor::{
    state_saving::{
        connect_to_database, create_tables_if_needed, read_room_compressor_progress,
        read_room_compressor_state, write_room_compressor_progress, write_room_compressor_state,
    },
    LevelState,
};
use serial_test::serial;

mod common;

#[test]
#[serial(db)]
fn write_then_read_state_gives_correct_results() {
    let mut client = connect_to_database(common::DB_URL).unwrap();
    create_tables_if_needed(&mut client).unwrap();
    common::empty_database();

    let room_id = "room1";
    let written_info: Vec<LevelState> = vec![(3, 1, Some(6)), (3, 2, Some(6))];
    write_room_compressor_state(&mut client, room_id, &written_info).unwrap();

    let read_info = read_room_compressor_state(&mut client, room_id)
        .unwrap()
        .unwrap();

    assert_eq!(written_info, read_info);
}

#[test]
#[serial(db)]
fn write_then_read_progress_gives_correct_results() {
    let mut client = connect_to_database(common::DB_URL).unwrap();
    create_tables_if_needed(&mut client).unwrap();
    common::empty_database();

    let room_id = "room1";
    let written_num = 53;
    write_room_compressor_progress(&mut client, room_id, written_num).unwrap();

    let read_num = read_room_compressor_progress(&mut client, room_id)
        .unwrap()
        .unwrap();

    assert_eq!(written_num, read_num);
}
