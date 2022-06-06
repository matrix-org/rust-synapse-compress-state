use crate::{
    compressor::{Compressor, Level, Stats},
    StateGroupEntry,
};
use state_map::StateMap;
use std::collections::BTreeMap;

#[test]
fn stats_correct_when_no_resets() {
    let mut initial: BTreeMap<i64, StateGroupEntry> = BTreeMap::new();
    let mut prev = None;

    // This starts with the following structure
    //
    // 0-1-2-3-4-5-6-7-8-9-10-11-12-13
    for i in 0i64..=13i64 {
        initial.insert(
            i,
            StateGroupEntry {
                in_range: true,
                prev_state_group: prev,
                state_map: StateMap::new(),
            },
        );

        prev = Some(i)
    }

    let mut compressor = Compressor {
        original_state_map: &initial,
        new_state_group_map: BTreeMap::new(),
        levels: vec![Level::new(3), Level::new(3)],
        stats: Stats::default(),
    };

    // This should create the following structure
    //
    // 0  3\      12
    // 1  4 6\    13
    // 2  5 7 9
    //      8 10
    //        11
    compressor.create_new_tree();

    // No resets should have taken place
    assert_eq!(compressor.stats.resets_no_suitable_prev, 0);
    assert_eq!(compressor.stats.resets_no_suitable_prev_size, 0);

    // Groups 3,6,9,12 should be the only ones changed
    assert_eq!(compressor.stats.state_groups_changed, 4);
}

#[test]
fn stats_correct_when_some_resets() {
    let mut initial: BTreeMap<i64, StateGroupEntry> = BTreeMap::new();
    let mut prev = None;

    // This starts with the following structure
    //
    // (note missing 3-4 link)
    // 0-1-2-3
    // 4-5-6-7-8-9-10-11-12-13
    //
    // Each group i has state:
    //     ('node','is',      i)
    //     ('group',  j, 'seen') where j is ancestor of i
    for i in 0i64..=13i64 {
        if i == 4 {
            prev = None
        }
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

    let mut compressor = Compressor {
        original_state_map: &initial,
        new_state_group_map: BTreeMap::new(),
        levels: vec![Level::new(3), Level::new(3)],
        stats: Stats::default(),
    };

    // This should create the following structure
    //
    // Brackets mean that has NO predecessor but is in that position in the
    // levels tree
    //
    // 0  3\        12
    // 1 (4)(6)\    13
    // 2  5  7  9
    //       8  10
    //          11
    compressor.create_new_tree();

    // the reset required for 4 contributes 2 to the size stat
    // - (1 'node' and 1 'group') entry
    // the reset required for 6 contributes 4 to the size stat
    // - (1 'node' and 3 'group') entry
    assert_eq!(compressor.stats.resets_no_suitable_prev, 2);
    assert_eq!(compressor.stats.resets_no_suitable_prev_size, 6);

    // groups 3,4,6,9,12 are the only ones changed
    assert_eq!(compressor.stats.state_groups_changed, 5);
}

#[test]
fn stats_correct_if_no_changes() {
    // This should create the following structure
    //
    // 0  3\      12
    // 1  4 6\    13
    // 2  5 7 9
    //      8 10
    //        11
    let initial_edges: BTreeMap<i64, i64> = vec![
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

    let mut initial: BTreeMap<i64, StateGroupEntry> = BTreeMap::new();

    for i in 0i64..=13i64 {
        // edge from map
        let prev = initial_edges.get(&i).copied();

        // insert that edge into the initial map
        initial.insert(
            i,
            StateGroupEntry {
                in_range: true,
                prev_state_group: prev,
                state_map: StateMap::new(),
            },
        );
    }

    let mut compressor = Compressor {
        original_state_map: &initial,
        new_state_group_map: BTreeMap::new(),
        levels: vec![Level::new(3), Level::new(3)],
        stats: Stats::default(),
    };

    // This should create the following structure (i.e. no change)
    //
    // 0  3\      12
    // 1  4 6\    13
    // 2  5 7 9
    //      8 10
    //        11
    compressor.create_new_tree();

    // No changes should have been made (the old tree should be the same)
    assert_eq!(compressor.stats.resets_no_suitable_prev, 0);
    assert_eq!(compressor.stats.resets_no_suitable_prev_size, 0);
    assert_eq!(compressor.stats.state_groups_changed, 0);
}
