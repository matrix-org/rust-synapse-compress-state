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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Level {
    /// The maximum size this level is allowed to be
    max_length: usize,
    /// The (approximate) current chain length of this level. This is equivalent
    /// to recursively following `current`
    current_chain_length: usize,
    /// The head of this level
    head: Option<i64>,
}

impl Level {
    /// Creates a new Level with the given maximum length
    pub fn new(max_length: usize) -> Level {
        Level {
            max_length,
            current_chain_length: 0,
            head: None,
        }
    }

    /// Creates a new level from stored state
    pub fn restore(max_length: usize, current_chain_length: usize, head: Option<i64>) -> Level {
        Level {
            max_length,
            current_chain_length,
            head,
        }
    }

    /// Update the current head of this level. If delta is true then it means
    /// that given state group will (probably) reference the previous head.
    ///
    /// Panics if `delta` is true and the level is already full.
    fn update(&mut self, new_head: i64, delta: bool) {
        self.head = Some(new_head);

        if delta {
            // If we're referencing the previous head then increment our chain
            // length estimate
            if !self.has_space() {
                panic!("Tried to add to an already full level");
            }

            self.current_chain_length += 1;
        } else {
            // Otherwise, we've started a new chain with a single entry.
            self.current_chain_length = 1;
        }
    }

    /// Get the max length of the level
    pub fn get_max_length(&self) -> usize {
        self.max_length
    }

    /// Get the current length of the level
    pub fn get_current_length(&self) -> usize {
        self.current_chain_length
    }

    /// Get the current head of the level
    pub fn get_head(&self) -> Option<i64> {
        self.head
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

    /// Creates a compressor and runs the compression algorithm.
    /// used when restoring compressor state from a previous run
    /// in which case the levels heads are also known
    pub fn compress_from_save(
        original_state_map: &'a BTreeMap<i64, StateGroupEntry>,
        level_info: &[Level],
    ) -> Compressor<'a> {
        let levels = level_info
            .iter()
            .map(|l| Level::restore((*l).max_length, (*l).current_chain_length, (*l).head))
            .collect();

        let mut compressor = Compressor {
            original_state_map,
            new_state_group_map: BTreeMap::new(),
            levels,
            stats: Stats::default(),
        };

        compressor.create_new_tree();
        compressor
    }

    /// Returns all the state required to save the compressor so it can be continued later
    pub fn get_level_info(&self) -> Vec<Level> {
        self.levels.clone()
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
                    prev_state_group = level.get_head();
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
        let state_map = collapse_state_maps(self.original_state_map, sg);

        let mut prev_sg = if let Some(prev_sg) = prev_sg {
            prev_sg
        } else {
            return (state_map, None);
        };

        // This is a loop to go through to find the first prev_sg which can be
        // a valid base for the state group.
        let mut prev_state_map;
        'outer: loop {
            prev_state_map = collapse_state_maps(self.original_state_map, prev_sg);
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
mod level_tests;

#[cfg(test)]
mod compressor_tests;

#[cfg(test)]
mod stats_tests;
