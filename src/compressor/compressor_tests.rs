use crate::{
    compressor::{Compressor, Level, Stats},
    StateGroupEntry,
};
use state_map::StateMap;
use std::collections::BTreeMap;
use string_cache::DefaultAtom as Atom;

#[test]
fn compress_creates_correct_compressor() {
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

    let compressor = Compressor::compress(&initial, &[3, 3]);

    let new_state = &compressor.new_state_group_map;

    // This should create the following structure
    //
    // 0  3\      12
    // 1  4 6\    13
    // 2  5 7 9
    //      8 10
    //        11
    let expected_edges: BTreeMap<i64, i64> = vec![
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

    for sg in 0i64..=13i64 {
        assert_eq!(
            expected_edges.get(&sg).cloned(),
            new_state[&sg].prev_state_group,
            "state group {} did not match expected",
            sg,
        );
    }
}

#[test]
fn create_new_tree_does_nothing_if_already_compressed() {
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
        let pred_group = initial_edges.get(&i);

        // Need Option<i64> not Option<&i64>
        let prev;
        match pred_group {
            Some(i) => prev = Some(*i),
            None => prev = None,
        }

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

    compressor.create_new_tree();

    let new_state = &compressor.new_state_group_map;

    assert_eq!(initial, *new_state);
}

#[test]
fn create_new_tree_respects_levels() {
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
    compressor.create_new_tree();

    let new_state = &compressor.new_state_group_map;

    // This should create the following structure
    //
    // 0  3\      12
    // 1  4 6\    13
    // 2  5 7 9
    //      8 10
    //        11
    let expected_edges: BTreeMap<i64, i64> = vec![
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

    for sg in 0i64..=13i64 {
        assert_eq!(
            expected_edges.get(&sg).cloned(),
            new_state[&sg].prev_state_group,
            "state group {} did not match expected",
            sg,
        );
    }
}

#[test]
#[should_panic(expected = "Can only call `create_new_tree` once")]
fn create_new_tree_panics_if_run_twice() {
    let mut initial: BTreeMap<i64, StateGroupEntry> = BTreeMap::new();
    let mut prev = None;

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
    compressor.create_new_tree();
    compressor.create_new_tree();
}

#[test]
fn create_new_tree_respects_all_not_in_range() {
    let mut initial: BTreeMap<i64, StateGroupEntry> = BTreeMap::new();
    let mut prev = None;

    // This starts with the following structure
    //
    // 0-1-2-3-4-5-6-7-8-9-10-11-12-13
    for i in 0i64..=13i64 {
        initial.insert(
            i,
            StateGroupEntry {
                in_range: false,
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
    compressor.create_new_tree();

    let new_state = &compressor.new_state_group_map;

    // This should create the following structure
    //
    // 0-1-2-3-4-5-6-7-8-9-10-11-12-13 (i.e. no change!)
    let expected_edges: BTreeMap<i64, i64> = vec![
        (1, 0),
        (2, 1),
        (3, 2),
        (4, 3),
        (5, 4),
        (6, 5),
        (7, 6),
        (8, 7),
        (9, 8),
        (10, 9),
        (11, 10),
        (12, 11),
        (13, 12),
    ]
    .into_iter()
    .collect();

    for sg in 0i64..=13i64 {
        assert_eq!(
            expected_edges.get(&sg).cloned(),
            new_state[&sg].prev_state_group,
            "state group {} did not match expected",
            sg,
        );
    }
}

#[test]
fn create_new_tree_respects_some_not_in_range() {
    let mut initial: BTreeMap<i64, StateGroupEntry> = BTreeMap::new();
    let mut prev = None;

    // This starts with the following structure
    //
    // 0-1-2-3-4-5-6-7-8-9-10-11-12-13-14-15-16-17-18
    for i in 0i64..=18i64 {
        initial.insert(
            i,
            StateGroupEntry {
                in_range: i > 4,
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
    compressor.create_new_tree();

    let new_state = &compressor.new_state_group_map;

    // This should create the following structure
    //
    // 0  5   8\       17
    // 1  6   9 11\    18
    // 2  7  10 12 14
    // 3        13 15
    // 4           16
    let expected_edges: BTreeMap<i64, i64> = vec![
        (1, 0),
        (2, 1),
        (3, 2),
        (4, 3), // No compression of nodes 0,1,2,3,4
        (6, 5), // Compresses in 3,3 leveling starting at 5
        (7, 6),
        (9, 8),
        (10, 9),
        (11, 8),
        (12, 11),
        (13, 12),
        (14, 11),
        (15, 14),
        (16, 15),
        (18, 17),
    ]
    .into_iter()
    .collect();
    for n in new_state {
        println!("{:?}", n);
    }

    for sg in 0i64..=13i64 {
        assert_eq!(
            expected_edges.get(&sg).cloned(),
            new_state[&sg].prev_state_group,
            "state group {} did not match expected",
            sg,
        );
    }
}

#[test]
fn create_new_tree_deals_with_impossible_preds() {
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
    compressor.create_new_tree();

    let new_state = &compressor.new_state_group_map;

    for n in new_state {
        println!("{:?}", n);
    }

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
    let expected_edges: BTreeMap<i64, i64> = vec![
        (1, 0),
        (2, 1),
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

    for sg in 0i64..=13i64 {
        assert_eq!(
            expected_edges.get(&sg).cloned(),
            new_state[&sg].prev_state_group,
            "state group {} did not match expected",
            sg,
        );
    }
}

#[test]
fn get_delta_returns_snapshot_if_no_prev_given() {
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

    // This should produce the following structure (tested above)
    //
    // 0  3\      12
    // 1  4 6\    13
    // 2  5 7 9
    //      8 10
    //        11
    //
    // State contents should be the same as before
    let mut compressor = Compressor::compress(&initial, &[3, 3]);

    let (found_delta, found_pred) = compressor.get_delta(None, 6);

    let mut expected_delta: StateMap<Atom> = StateMap::new();
    expected_delta.insert("node", "is", "6".into());
    expected_delta.insert("group", "0", "seen".into());
    expected_delta.insert("group", "1", "seen".into());
    expected_delta.insert("group", "2", "seen".into());
    expected_delta.insert("group", "3", "seen".into());
    expected_delta.insert("group", "4", "seen".into());
    expected_delta.insert("group", "5", "seen".into());
    expected_delta.insert("group", "6", "seen".into());

    assert_eq!(found_delta, expected_delta);
    assert_eq!(found_pred, None);
}

#[test]
fn get_delta_returns_delta_if_original_predecessor() {
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

    // This should produce the following structure (tested above)
    //
    // 0  3\      12
    // 1  4 6\    13
    // 2  5 7 9
    //      8 10
    //        11
    //
    // State contents should be the same as before
    let mut compressor = Compressor::compress(&initial, &[3, 3]);

    let (found_delta, found_pred) = compressor.get_delta(Some(5), 6);

    let mut expected_delta: StateMap<Atom> = StateMap::new();
    expected_delta.insert("node", "is", "6".into());
    expected_delta.insert("group", "6", "seen".into());

    assert_eq!(found_delta, expected_delta);
    assert_eq!(found_pred, Some(5));
}

#[test]
fn get_delta_returns_delta_if_original_multi_hop_predecessor() {
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

    // This should produce the following structure (tested above)
    //
    // 0  3\      12
    // 1  4 6\    13
    // 2  5 7 9
    //      8 10
    //        11
    //
    // State contents should be the same as before
    let mut compressor = Compressor::compress(&initial, &[3, 3]);

    let (found_delta, found_pred) = compressor.get_delta(Some(3), 6);

    let mut expected_delta: StateMap<Atom> = StateMap::new();
    expected_delta.insert("node", "is", "6".into());
    expected_delta.insert("group", "4", "seen".into());
    expected_delta.insert("group", "5", "seen".into());
    expected_delta.insert("group", "6", "seen".into());

    assert_eq!(found_delta, expected_delta);
    assert_eq!(found_pred, Some(3));
}

#[test]
fn get_delta_returns_snapshot_if_no_prev_possible() {
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
        // don't add 3-4 link
        if i == 4 {
            prev = None
        }

        // populate the delta for this state
        let mut entry = StateGroupEntry {
            in_range: true,
            prev_state_group: prev,
            state_map: StateMap::new(),
        };
        entry
            .state_map
            .insert("group", &i.to_string(), "seen".into());
        entry.state_map.insert("node", "is", i.to_string().into());

        // put the entry into the initial map
        initial.insert(i, entry);

        prev = Some(i)
    }

    // This should create the following structure if create_new_tree() was run
    // (tested in create_new_tree_deals_with_impossible_preds())
    //
    // Brackets mean that has NO predecessor but is in that position in the
    // levels tree
    //
    // 0  3\        12
    // 1 (4)(6)\    13
    // 2  5  7  9
    //       8  10
    //          11
    //
    // State contents should be the same as before

    // build up new_tree after 0,1,2,3 added
    let mut new_map: BTreeMap<i64, StateGroupEntry> = BTreeMap::new();

    // 0-1-2 is left the same
    new_map.insert(0, initial.get(&0).unwrap().clone());
    new_map.insert(1, initial.get(&1).unwrap().clone());
    new_map.insert(2, initial.get(&2).unwrap().clone());

    // 3 is now a snapshot
    let mut entry_3: StateMap<Atom> = StateMap::new();
    entry_3.insert("node", "is", "3".into());
    entry_3.insert("group", "0", "seen".into());
    entry_3.insert("group", "1", "seen".into());
    entry_3.insert("group", "2", "seen".into());
    entry_3.insert("group", "3", "seen".into());
    new_map.insert(
        3,
        StateGroupEntry {
            in_range: true,
            prev_state_group: None,
            state_map: entry_3,
        },
    );

    // build the compressor with this partialy built new map
    let mut compressor = Compressor {
        original_state_map: &initial,
        new_state_group_map: new_map,
        levels: vec![Level::new(3), Level::new(3)],
        stats: Stats::default(),
    };

    // make the levels how they would be after 0,1,2,3 added
    // they should both be of length 1 and have 3 as the current head
    let mut levels_iter = compressor.levels.iter_mut();

    let l1 = levels_iter.next().unwrap();
    l1.head = Some(3);
    l1.current_chain_length = 1;

    let l2 = levels_iter.next().unwrap();
    l2.head = Some(3);
    l2.current_chain_length = 1;

    // Now try and find delta for 4 with 3 as pred
    let (found_delta, found_pred) = compressor.get_delta(Some(3), 4);

    let mut expected_delta: StateMap<Atom> = StateMap::new();
    expected_delta.insert("node", "is", "4".into());
    expected_delta.insert("group", "4", "seen".into());

    assert_eq!(found_delta, expected_delta);
    assert_eq!(found_pred, None);
}
