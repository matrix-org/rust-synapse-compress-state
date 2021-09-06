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
