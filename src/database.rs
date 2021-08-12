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

use indicatif::{ProgressBar, ProgressStyle};
use openssl::ssl::{SslConnector, SslMethod, SslVerifyMode};
use postgres::{fallible_iterator::FallibleIterator, types::ToSql, Client};
use postgres_openssl::MakeTlsConnector;
use rand::{distributions::Alphanumeric, thread_rng, Rng};
use std::{borrow::Cow, collections::BTreeMap, fmt};

use super::StateGroupEntry;

/// Fetch the entries in state_groups_state (and their prev groups) for a
/// specific room.
///
/// - Connects to the database
/// - Fetches the first [group] rows with group id after [min]
/// - Recursively searches for missing predecessors and adds those
///
/// Returns with the state_group map and the id of the last group that was used
///
/// # Arguments
///
/// * `room_id`             -   The ID of the room in the database
/// * `db_url`              -   The URL of a Postgres database. This should be of the
///                             form: "postgresql://user:pass@domain:port/database"
/// * `min_state_group`     -   If specified, then only fetch the entries for state
///                             groups greater than (but not equal) to this number. It
///                             also requires groups_to_compress to be specified
/// * 'groups_to_compress'  -   The number of groups to get from the database before stopping
/// * `max_state_group`     -   If specified, then only fetch the entries for state
///                             groups lower than or equal to this number.
pub fn get_data_from_db(
    db_url: &str,
    room_id: &str,
    min_state_group: Option<i64>,
    groups_to_compress: Option<i64>,
    max_state_group: Option<i64>,
) -> (BTreeMap<i64, StateGroupEntry>, i64) {
    // connect to the database
    let mut builder = SslConnector::builder(SslMethod::tls()).unwrap();
    builder.set_verify(SslVerifyMode::NONE);
    let connector = MakeTlsConnector::new(builder.build());

    let mut client = Client::connect(db_url, connector).unwrap();

    // Search for the group id of the groups_to_compress'th group after min_state_group
    // If this is saved, then the compressor can continue by having min_state_group being
    // set to this maximum
    let max_group_found = find_max_group(
        &mut client,
        room_id,
        min_state_group,
        groups_to_compress,
        max_state_group,
    );

    let mut state_group_map =
        get_initial_data_from_db(&mut client, room_id, min_state_group, max_group_found);

    println!("Got initial state from database. Checking for any missing state groups...");

    // Due to reasons some of the state groups appear in the edges table, but
    // not in the state_groups_state table.
    //
    // Also it is likely that the predecessor of a node will not be within the
    // chunk that was specified by min_state_group and groups_to_compress.
    // This means they don't get included in our DB queries, so we have to fetch
    // any missing groups explicitly.
    //
    // Since the returned groups may themselves reference groups we don't have,
    // we need to do this recursively until we don't find any more missing.
    loop {
        let mut missing_sgs: Vec<_> = state_group_map
            .iter()
            .filter_map(|(_sg, entry)| {
                if let Some(prev_sg) = entry.prev_state_group {
                    if state_group_map.contains_key(&prev_sg) {
                        None
                    } else {
                        Some(prev_sg)
                    }
                } else {
                    None
                }
            })
            .collect();

        if missing_sgs.is_empty() {
            // println!("No missing state groups");
            break;
        }

        missing_sgs.sort_unstable();
        missing_sgs.dedup();

        // println!("Missing {} state groups", missing_sgs.len());

        // find state groups not picked up already and add them to the map
        let map = get_missing_from_db(&mut client, &missing_sgs, min_state_group, max_group_found);
        for (k, v) in map {
            state_group_map.entry(k).or_insert(v);
        }
    }

    (state_group_map, max_group_found)
}

/// Returns the group ID of the last group to be compressed
///
/// This can be saved so that future runs of the compressor only
/// continue from after this point
///
/// # Arguments
///
/// * `client`              -   A Postgres client to make requests with
/// * `room_id`             -   The ID of the room in the database
/// * `min_state_group`     -   The lower limit (non inclusive) of group id's to compress
/// * 'groups_to_compress'  -   How many groups to compress
/// * `max_state_group`     -   The upper bound on what this method can return
fn find_max_group(
    client: &mut Client,
    room_id: &str,
    min_state_group: Option<i64>,
    groups_to_compress: Option<i64>,
    max_state_group: Option<i64>,
) -> i64 {
    // Get list of state_id's in a certain room
    let mut query_chunk_of_ids = "SELECT id FROM state_groups WHERE room_id = $1".to_string();
    let params: Vec<&(dyn ToSql + Sync)>;

    if let Some(max) = max_state_group {
        query_chunk_of_ids = format!("{} AND id <= {}", query_chunk_of_ids, max)
    }

    // Adds additional constraint if a groups_to_compress has been specified
    if min_state_group.is_some() && groups_to_compress.is_some() {
        params = vec![&room_id, &min_state_group, &groups_to_compress];
        query_chunk_of_ids = format!(r"{} AND id > $2 LIMIT $3", query_chunk_of_ids);
    } else {
        params = vec![&room_id];
        query_chunk_of_ids = format!(r"{} ORDER BY id DESC LIMIT 1", query_chunk_of_ids);
    }

    let sql_query = format!(
        "SELECT id FROM ({}) AS ids ORDER BY ids.id DESC LIMIT 1",
        query_chunk_of_ids
    );
    let final_row = client.query(sql_query.as_str(), &params).unwrap();

    final_row.last().unwrap().get(0)
}

/// Fetch the entries in state_groups_state and immediate predecessors for
/// a specific room.
///
/// - Fetches first [groups_to_compress] rows with group id higher than min
/// - Stores the group id, predecessor id and deltas into a map
/// - returns map and maximum row that was considered
///
/// # Arguments
///
/// * `client`          -   A Postgres client to make requests with
/// * `room_id`         -   The ID of the room in the database
/// * `min_state_group` -   If specified, then only fetch the entries for state
///                         groups greater than (but not equal) to this number. It
///                         also requires groups_to_compress to be specified
/// * 'max_group_found' -   The upper limit on state_groups ids to get from the database
fn get_initial_data_from_db(
    client: &mut Client,
    room_id: &str,
    min_state_group: Option<i64>,
    max_group_found: i64,
) -> BTreeMap<i64, StateGroupEntry> {
    // Query to get id, predecessor and deltas for each state group
    let sql = r#"
        SELECT m.id, prev_state_group, type, state_key, s.event_id
        FROM state_groups AS m
        LEFT JOIN state_groups_state AS s ON (m.id = s.state_group)
        LEFT JOIN state_group_edges AS e ON (m.id = e.state_group)
        WHERE m.room_id = $1
    "#;

    // Adds additional constraint if minimum state_group has been specified.
    // note that the maximum group only affects queries if there is also a minimum
    // otherwise it is assumed that ALL groups should be fetched
    let mut rows = if let Some(min) = min_state_group {
        let params: Vec<&dyn ToSql> = vec![&room_id, &min, &max_group_found];
        client.query_raw(
            format!(r"{} AND m.id > $2 AND m.id <= $3", sql).as_str(),
            params,
        )
    } else {
        client.query_raw(sql, &[room_id])
    }
    .unwrap();

    // Copy the data from the database into a map
    let mut state_group_map: BTreeMap<i64, StateGroupEntry> = BTreeMap::new();

    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner().template("{spinner} [{elapsed}] {pos} rows retrieved"),
    );
    pb.enable_steady_tick(100);

    while let Some(row) = rows.next().unwrap() {
        // The row in the map to copy the data to
        let entry = state_group_map.entry(row.get(0)).or_default();

        // Save the predecessor and mark for compression (this may already be there)
        // TODO: slightly fewer redundant rewrites
        entry.prev_state_group = row.get(1);
        entry.in_range = true;

        // Copy the single delta from the predecessor stored in this row
        if let Some(etype) = row.get::<_, Option<String>>(2) {
            entry.state_map.insert(
                &etype,
                &row.get::<_, String>(3),
                row.get::<_, String>(4).into(),
            );
        }

        pb.inc(1);
    }

    pb.set_length(pb.position());
    pb.finish();

    state_group_map
}

/// Finds the predecessors of missing state groups
///
/// N.B. this does NOT find their deltas
///
/// # Arguments
///
/// * `client`          -   A Postgres client to make requests with
/// * `missing_sgs`     -   An array of missing state_group ids
/// * 'min_state_group' -   Minimum state_group id to mark as in range
/// * 'max_group_found' -   Maximum state_group id to mark as in range
fn get_missing_from_db(
    client: &mut Client,
    missing_sgs: &[i64],
    min_state_group: Option<i64>,
    max_group_found: i64,
) -> BTreeMap<i64, StateGroupEntry> {
    // "Due to reasons" it is possible that some states only appear in edges table and not in state_groups table
    // so since we know the IDs we're looking for as they are the missing predecessors, we can find them by
    // left joining onto the edges table (instead of the state_group table!)
    let sql = r#"
        SELECT target.prev_state_group, source.prev_state_group, state.type, state.state_key, state.event_id
        FROM state_group_edges AS target
        LEFT JOIN state_group_edges AS source ON (target.prev_state_group = source.state_group)
        LEFT JOIN state_groups_state AS state ON (target.prev_state_group = state.state_group)
        WHERE target.prev_state_group = ANY($1)
    "#;

    let mut rows = client.query_raw(sql, &[missing_sgs]).unwrap();

    let mut state_group_map: BTreeMap<i64, StateGroupEntry> = BTreeMap::new();

    while let Some(row) = rows.next().unwrap() {
        let id = row.get(0);
        // The row in the map to copy the data to
        let entry = state_group_map.entry(id).or_default();

        // Save the predecessor and mark for compression (this may already be there)
        // Also may well not exist!
        entry.prev_state_group = row.get(1);
        if let Some(min) = min_state_group {
            if min < id && id <= max_group_found {
                entry.in_range = true
            }
        }

        // Copy the single delta from the predecessor stored in this row
        if let Some(etype) = row.get::<_, Option<String>>(2) {
            entry.state_map.insert(
                &etype,
                &row.get::<_, String>(3),
                row.get::<_, String>(4).into(),
            );
        }
    }

    state_group_map
}

// TODO: find a library that has an existing safe postgres escape function
/// Helper function that escapes the wrapped text when writing SQL
pub struct PGEscape<'a>(pub &'a str);

impl<'a> fmt::Display for PGEscape<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut delim = Cow::from("$$");
        while self.0.contains(&delim as &str) {
            let s: String = thread_rng()
                .sample_iter(&Alphanumeric)
                .take(10)
                .map(char::from)
                .collect();

            delim = format!("${}$", s).into();
        }

        write!(f, "{}{}{}", delim, self.0, delim)
    }
}

#[test]
fn test_pg_escape() {
    let s = format!("{}", PGEscape("test"));
    assert_eq!(s, "$$test$$");

    let dodgy_string = "test$$ing";

    let s = format!("{}", PGEscape(dodgy_string));

    // prefix and suffixes should match
    let start_pos = s.find(dodgy_string).expect("expected to find dodgy string");
    let end_pos = start_pos + dodgy_string.len();
    assert_eq!(s[..start_pos], s[end_pos..]);

    // .. and they should start and end with '$'
    assert_eq!(&s[0..1], "$");
    assert_eq!(&s[start_pos - 1..start_pos], "$");
}
