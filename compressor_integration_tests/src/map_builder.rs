use std::collections::BTreeMap;

use state_map::StateMap;
use synapse_compress_state::StateGroupEntry;

/// Generates long chain of state groups each with state deltas
///
/// If called wiht start=0, end=13 this would build the following:
///
/// 0-1-2-3-4-5-6-7-8-9-10-11-12-13
///
/// Where each group i has state:
///     ('node','is',      i)
///     ('group',  j, 'seen') - for all j less than i
pub fn line_with_state(start: i64, end: i64) -> BTreeMap<i64, StateGroupEntry> {
    let mut initial: BTreeMap<i64, StateGroupEntry> = BTreeMap::new();
    let mut prev = None;

    for i in start..=end {
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

    initial
}

/// Generates line segments in a chain of state groups each with state deltas
///
/// If called wiht start=0, end=13 this would build the following:
///
/// 0-1-2 3-4-5 6-7-8 9-10-11 12-13
///
/// Where each group i has state:
///     ('node','is',      i)
///     ('group',  j, 'seen') - for all j less than i
pub fn line_segments_with_state(start: i64, end: i64) -> BTreeMap<i64, StateGroupEntry> {
    let mut initial: BTreeMap<i64, StateGroupEntry> = BTreeMap::new();
    let mut prev = None;

    for i in start..=end {
        // if the state is a snapshot then set its predecessor to NONE
        if (i - start) % 3 == 0 {
            prev = None;
        }

        // create a blank entry for it
        let mut entry = StateGroupEntry {
            in_range: true,
            prev_state_group: prev,
            state_map: StateMap::new(),
        };

        // if it's a snapshot then add in all previous state
        if prev.is_none() {
            for j in start..i {
                entry
                    .state_map
                    .insert("group", &j.to_string(), "seen".into());
            }
        }

        // add in the new state for this state group
        entry
            .state_map
            .insert("group", &i.to_string(), "seen".into());
        entry.state_map.insert("node", "is", i.to_string().into());

        // put it into the initial map
        initial.insert(i, entry);

        // set this group as the predecessor for the next
        prev = Some(i)
    }
    initial
}

/// This generates the correct compressed structure with 3,3 levels
///
/// Note: only correct structure when no impossible predecessors
///
/// Structure generated:
///
/// 0  3\      12
/// 1  4 6\    13
/// 2  5 7 9
///     8 10
///        11
/// Where each group i has state:
///     ('node','is',      i)
///     ('group',  j, 'seen') - for all j less than i
pub fn compressed_3_3_from_0_to_13_with_state() -> BTreeMap<i64, StateGroupEntry> {
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

    let mut expected: BTreeMap<i64, StateGroupEntry> = BTreeMap::new();

    // Each group i has state:
    //     ('node','is',      i)
    //     ('group',  j, 'seen') - for all j less than i
    for i in 0i64..=13i64 {
        let prev = expected_edges.get(&i);

        //change from Option<&i64> to Option<i64>
        let prev = prev.copied();

        // create a blank entry for it
        let mut entry = StateGroupEntry {
            in_range: true,
            prev_state_group: prev,
            state_map: StateMap::new(),
        };

        // Add in all state between predecessor and now (non inclusive)
        if let Some(p) = prev {
            for j in (p + 1)..i {
                entry
                    .state_map
                    .insert("group", &j.to_string(), "seen".into());
            }
        } else {
            for j in 0i64..i {
                entry
                    .state_map
                    .insert("group", &j.to_string(), "seen".into());
            }
        }

        // add in the new state for this state group
        entry
            .state_map
            .insert("group", &i.to_string(), "seen".into());
        entry.state_map.insert("node", "is", i.to_string().into());

        // put it into the expected map
        expected.insert(i, entry);
    }
    expected
}
