// Copyright 2018 New Vector Ltd
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! This is the actual compression algorithm.
//!
//! The algorithm attempts to make a tree of deltas for the state group maps.
//! This is done by having multiple "levels", where each level has a maximum
//! size. The state groups are iterated over, with deltas being calculated
//! against the smallest level that isn't yet full. When a state group is
//! inserted into a level, or lower levels are reset to have their current
//! "head" at the new state group.
//!
//! This produces graphs that look roughly like, for two levels:
//!
//! ```ignore
//! L2 <-------------------- L2 <---------- ...
//!  ^--- L1 <--- L1 <--- L1  ^--- L1 <--- L1 <--- L1
//! ```

use indicatif::{ProgressBar, ProgressStyle};
use state_map::StateMap;
use std::collections::BTreeMap;
use string_cache::DefaultAtom as Atom;

use super::{collapse_state_maps, StateGroupEntry};

/// Holds information about a particular level.
#[derive(Debug)]
struct Level {
    /// The maximum size this level is allowed to be
    max_length: usize,
    /// The (approximate) current chain length of this level. This is equivalent
    /// to recursively following `current`
    current_chain_length: usize,
    /// The head of this level
    current: Option<i64>,
}

impl Level {
    /// Creates a new Level with the given maximum length
    pub fn new(max_length: usize) -> Level {
        Level {
            max_length,
            current_chain_length: 0,
            current: None,
        }
    }

    /// Update the current head of this level. If delta is true then it means
    /// that given state group will (probably) reference the previous head.
    ///
    /// Panics if `delta` is true and the level is already full.
    pub fn update(&mut self, current: i64, delta: bool) {
        self.current = Some(current);

        if delta {
            // If we're referencing the previous head then increment our chain
            // length estimate
            if !self.has_space() {
                panic!("Tried to add to a already full level");
            }

            self.current_chain_length += 1;
        } else {
            // Otherwise, we've started a new chain with a single entry.
            self.current_chain_length = 1;
        }
    }

    /// Get the current head of the level
    pub fn get_current(&self) -> Option<i64> {
        self.current
    }

    /// Whether there is space in the current chain at this level. If not then a
    /// new chain should be started.
    pub fn has_space(&self) -> bool {
        self.current_chain_length < self.max_length
    }
}

/// Keeps track of some statistics of a compression run.
#[derive(Default)]
pub struct Stats {
    /// How many state groups we couldn't find a delta for, despite trying.
    pub resets_no_suitable_prev: usize,
    /// The sum of the rows of the state groups counted by
    /// `resets_no_suitable_prev`.
    pub resets_no_suitable_prev_size: usize,
    /// How many state groups we have changed.
    pub state_groups_changed: usize,
}

/// Attempts to compress a set of state deltas using the given level sizes.
pub struct Compressor<'a> {
    original_state_map: &'a BTreeMap<i64, StateGroupEntry>,
    pub new_state_group_map: BTreeMap<i64, StateGroupEntry>,
    levels: Vec<Level>,
    pub stats: Stats,
}

impl<'a> Compressor<'a> {
    /// Creates a compressor and runs the compression algorithm.
    pub fn compress(
        original_state_map: &'a BTreeMap<i64, StateGroupEntry>,
        level_sizes: &[usize],
    ) -> Compressor<'a> {
        let mut compressor = Compressor {
            original_state_map,
            new_state_group_map: BTreeMap::new(),
            levels: level_sizes.iter().map(|size| Level::new(*size)).collect(),
            stats: Stats::default(),
        };

        compressor.create_new_tree();

        compressor
    }

    /// Actually runs the compression algorithm
    fn create_new_tree(&mut self) {
        if !self.new_state_group_map.is_empty() {
            panic!("Can only call `create_new_tree` once");
        }

        let pb = ProgressBar::new(self.original_state_map.len() as u64);
        pb.set_style(
            ProgressStyle::default_bar().template("[{elapsed_precise}] {bar} {pos}/{len} {msg}"),
        );
        pb.set_message("state groups");
        pb.enable_steady_tick(100);

        for (&state_group, entry) in self.original_state_map {
            // Check whether this entry is in_range or is just present in the map due to being
            // a predecessor of a group that IS in_range for compression
            if !entry.in_range {
                let new_entry = StateGroupEntry {
                    // in_range is kept the same so that the new entry is equal to the old entry
                    // otherwise it might trigger a useless database transaction
                    in_range: entry.in_range,
                    prev_state_group: entry.prev_state_group,
                    state_map: entry.state_map.clone(),
                };
                // Paranoidly assert that not making changes to this entry
                // could probably be removed...
                assert!(new_entry == *entry);
                self.new_state_group_map.insert(state_group, new_entry);

                continue;
            }
            let mut prev_state_group = None;
            for level in &mut self.levels {
                if level.has_space() {
                    prev_state_group = level.get_current();
                    level.update(state_group, true);
                    break;
                } else {
                    level.update(state_group, false);
                }
            }

            let (delta, prev_state_group) = if entry.prev_state_group == prev_state_group {
                (entry.state_map.clone(), prev_state_group)
            } else {
                self.stats.state_groups_changed += 1;
                self.get_delta(prev_state_group, state_group)
            };

            self.new_state_group_map.insert(
                state_group,
                StateGroupEntry {
                    in_range: true,
                    prev_state_group,
                    state_map: delta,
                },
            );

            pb.inc(1);
        }

        pb.finish();
    }

    /// Attempts to calculate the delta between two state groups.
    ///
    /// This is not always possible if the given candidate previous state group
    /// have state keys that are not in the new state group. In this case the
    /// function will try and iterate back up the current tree to find a state
    /// group that can be used as a base for a delta.
    ///
    /// Returns the state map and the actual base state group (if any) used.
    fn get_delta(&mut self, prev_sg: Option<i64>, sg: i64) -> (StateMap<Atom>, Option<i64>) {
        let state_map = collapse_state_maps(&self.original_state_map, sg);

        let mut prev_sg = if let Some(prev_sg) = prev_sg {
            prev_sg
        } else {
            return (state_map, None);
        };

        // This is a loop to go through to find the first prev_sg which can be
        // a valid base for the state group.
        let mut prev_state_map;
        'outer: loop {
            prev_state_map = collapse_state_maps(&self.original_state_map, prev_sg);
            for (t, s) in prev_state_map.keys() {
                if !state_map.contains_key(t, s) {
                    // This is not a valid base as it contains key the new state
                    // group doesn't have. Attempt to walk up the tree to find a
                    // better base.
                    if let Some(psg) = self.new_state_group_map[&prev_sg].prev_state_group {
                        prev_sg = psg;
                        continue 'outer;
                    }

                    // Couldn't find a new base, so we give up and just persist
                    // a full state group here.
                    self.stats.resets_no_suitable_prev += 1;
                    self.stats.resets_no_suitable_prev_size += state_map.len();

                    return (state_map, None);
                }
            }

            break;
        }

        // We've found a valid base, now we just need to calculate the delta.
        let mut delta_map = StateMap::new();

        for ((t, s), e) in state_map.iter() {
            if prev_state_map.get(t, s) != Some(e) {
                delta_map.insert(t, s, e.clone());
            }
        }

        (delta_map, Some(prev_sg))
    }
}

#[cfg(test)]
mod level_tests {
    use crate::compressor::Level;
    #[test]
    fn new_produces_empty_level() {
        let l = Level::new(15);
        assert_eq!(l.max_length, 15);
        assert_eq!(l.current_chain_length, 0);
        assert_eq!(l.current, None);
    }

    #[test]
    fn update_adds_to_non_full_level() {
        let mut l = Level::new(10);
        l.update(7, true);
        assert_eq!(l.max_length, 10);
        assert_eq!(l.current_chain_length, 1);
        assert_eq!(l.current, Some(7));
    }

    #[test]
    #[should_panic]
    fn update_panics_if_adding_and_too_full() {
        let mut l = Level::new(5);
        l.update(1, true);
        l.update(2, true);
        l.update(3, true);
        l.update(4, true);
        l.update(5, true);
        l.update(6, true);
    }

    #[test]
    fn update_resets_level_correctly() {
        let mut l = Level::new(5);
        l.update(1, true);
        l.update(2, true);
        l.update(3, true);
        l.update(4, true);
        l.update(5, true);
        l.update(6, false);
        assert_eq!(l.max_length, 5);
        assert_eq!(l.current_chain_length, 1);
        assert_eq!(l.current, Some(6));
    }

    #[test]
    fn get_current_returns_current() {
        let mut l = Level::new(5);
        assert_eq!(l.get_current(), None);
        l.update(23, true);
        assert_eq!(l.get_current(), Some(23));
    }

    #[test]
    fn has_space_returns_true_if_empty() {
        let l = Level::new(15);
        assert_eq!(l.has_space(), true);
    }

    #[test]
    fn has_space_returns_true_if_part_full() {
        let mut l = Level::new(15);
        l.update(12, true);
        l.update(234, true);
        l.update(1, true);
        l.update(143, true);
        l.update(15, true);
        assert_eq!(l.has_space(), true);
    }

    #[test]
    fn has_space_returns_false_if_full() {
        let mut l = Level::new(5);
        l.update(1, true);
        l.update(2, true);
        l.update(3, true);
        l.update(4, true);
        l.update(5, true);
        assert_eq!(l.has_space(), false);
    }
}

#[cfg(test)]
mod compressor_tests {
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
    #[should_panic]
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
        l1.current = Some(3);
        l1.current_chain_length = 1;

        let l2 = levels_iter.next().unwrap();
        l2.current = Some(3);
        l2.current_chain_length = 1;

        // Now try and find delta for 4 with 3 as pred
        let (found_delta, found_pred) = compressor.get_delta(Some(3), 4);

        let mut expected_delta: StateMap<Atom> = StateMap::new();
        expected_delta.insert("node", "is", "4".into());
        expected_delta.insert("group", "4", "seen".into());

        assert_eq!(found_delta, expected_delta);
        assert_eq!(found_pred, None);
    }
}

#[cfg(test)]
mod stats_tests {
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
}
