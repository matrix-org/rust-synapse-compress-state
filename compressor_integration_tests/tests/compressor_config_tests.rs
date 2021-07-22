use compressor_integration_tests::{
    add_contents_to_database, empty_database, map_builder::line_with_state, DB_URL,
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
