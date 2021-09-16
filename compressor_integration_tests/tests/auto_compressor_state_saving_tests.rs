use auto_compressor::state_saving::{
    connect_to_database, create_tables_if_needed, read_room_compressor_state,
    write_room_compressor_state,
};
use compressor_integration_tests::{clear_compressor_state, DB_URL};
use serial_test::serial;
use synapse_compress_state::Level;

#[test]
#[serial(db)]
fn write_then_read_state_gives_correct_results() {
    let mut client = connect_to_database(DB_URL).unwrap();
    create_tables_if_needed(&mut client).unwrap();
    clear_compressor_state();

    let room_id = "room1";
    let written_info: Vec<Level> =
        vec![Level::restore(3, 1, Some(6)), Level::restore(3, 2, Some(6))];
    let written_num = 53;
    write_room_compressor_state(&mut client, room_id, &written_info, written_num).unwrap();

    let (read_num, read_info) = read_room_compressor_state(&mut client, room_id)
        .unwrap()
        .unwrap();

    assert_eq!(written_info, read_info);
    assert_eq!(written_num, read_num);
}
