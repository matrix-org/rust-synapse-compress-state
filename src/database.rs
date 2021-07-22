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
use postgres::{fallible_iterator::FallibleIterator, Client};
use postgres_openssl::MakeTlsConnector;
use rand::{distributions::Alphanumeric, thread_rng, Rng};
use std::{borrow::Cow, collections::BTreeMap, fmt};

use super::StateGroupEntry;

/// Fetch the entries in state_groups_state (and their prev groups) for a
/// specific room.
///
/// - Connects to the database
/// - Recursively searches for missing predecessors and adds those
///
/// # Arguments
///
/// * `room_id`         -   The ID of the room in the database
/// * `db_url`          -   The URL of a Postgres database. This should be of the
///                         form: "postgresql://user:pass@domain:port/database"
pub fn get_data_from_db(
    db_url: &str,
    room_id: &str,
) -> BTreeMap<i64, StateGroupEntry> {
    let mut builder = SslConnector::builder(SslMethod::tls()).unwrap();
    builder.set_verify(SslVerifyMode::NONE);
    let connector = MakeTlsConnector::new(builder.build());

    let mut client = Client::connect(db_url, connector).unwrap();

    let mut state_group_map = get_initial_data_from_db(&mut client, room_id);

    println!("Got initial state from database. Checking for any missing state groups...");

    // Due to reasons some of the state groups appear in the edges table, but
    // not in the state_groups_state table. This means they don't get included
    // in our DB queries, so we have to fetch any missing groups explicitly.
    // Since the returned groups may themselves reference groups we don't have,
    // we need to do this recursively until we don't find any more missing.
    //
    // N.B. This does NOT currently fetch the deltas for the missing groups!
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
            println!("No missing state groups");
            break;
        }

        missing_sgs.sort_unstable();
        missing_sgs.dedup();

        println!("Missing {} state groups", missing_sgs.len());

        let map = get_missing_from_db(&mut client, &missing_sgs);
        state_group_map.extend(map.into_iter());
    }

    state_group_map
}

/// Fetch the entries in state_groups_state and immediate predecessors for
/// a specific room.
///
/// - Fetches rows with group id lower than max
/// - Stores the group id, predecessor id and deltas into a map
///
/// # Arguments
///
/// * `client`          -   A Postgres client to make requests with
/// * `room_id`         -   The ID of the room in the database
/// * `max_state_group` -   If specified, then only fetch the entries for state
///                         groups lower than or equal to this number. (N.B. doesn't
///                         fetch IMMEDIATE predecessors if ID is above this number)
fn get_initial_data_from_db(
    client: &mut Client,
    room_id: &str,
) -> BTreeMap<i64, StateGroupEntry> {
    // Query to get id, predecessor and delta for each state group
    let sql = r#"
        SELECT m.id, prev_state_group, type, state_key, s.event_id
        FROM state_groups AS m
        LEFT JOIN state_groups_state AS s ON (m.id = s.state_group)
        LEFT JOIN state_group_edges AS e ON (m.id = e.state_group)
        WHERE m.room_id = $1
    "#;

    // Adds additional constraint if a max_state_group has been specified
    // Then sends query to the datatbase
    let mut rows = 
        client.query_raw(sql, &[room_id])
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
fn get_missing_from_db(client: &mut Client, missing_sgs: &[i64]) -> BTreeMap<i64, StateGroupEntry> {
    let mut rows = client
        .query_raw(
            r#"
                SELECT state_group, prev_state_group
                FROM state_group_edges
                WHERE state_group = ANY($1)
            "#,
            &[missing_sgs],
        )
        .unwrap();

    // initialise the map with empty entries (the missing group may not
    // have a prev_state_group either)
    let mut state_group_map: BTreeMap<i64, StateGroupEntry> = missing_sgs
        .iter()
        .map(|sg| (*sg, StateGroupEntry::default()))
        .collect();

    while let Some(row) = rows.next().unwrap() {
        let state_group = row.get(0);
        let entry = state_group_map.get_mut(&state_group).unwrap();
        entry.prev_state_group = row.get(1);
    }

    state_group_map
}

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
