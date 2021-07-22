use std::collections::BTreeMap;

use serial_test::serial;
use state_map::StateMap;
use synapse_compress_state::{run, Config, StateGroupEntry};

mod common;

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

    let db_url = "postgresql://synapse_user:synapse_pass@localhost/synapse".to_string();
    let output_file = "./tests/tmp/output.sql".to_string();
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
