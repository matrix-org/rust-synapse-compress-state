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

use fallible_iterator::FallibleIterator;
use indicatif::{ProgressBar, ProgressStyle};
use postgres::{Connection, TlsMode};
use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt;

use StateGroupEntry;

/// Fetch the entries in state_groups_state (and their prev groups) for the
/// given `room_id` by connecting to the postgres database at `db_url`.
pub fn get_data_from_db(
    db_url: &str,
    room_id: &str,
    max_state_group: Option<i64>,
) -> BTreeMap<i64, StateGroupEntry> {
    let conn = Connection::connect(db_url, TlsMode::None).unwrap();

    let mut state_group_map = get_initial_data_from_db(&conn, room_id, max_state_group);

    println!("Got initial state from database. Checking for any missing state groups...");

    // Due to reasons some of the state groups appear in the edges table, but
    // not in the state_groups_state table. This means they don't get included
    // in our DB queries, so we have to fetch any missing groups explicitly.
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
            println!("No missing state groups");
            break;
        }

        missing_sgs.sort_unstable();
        missing_sgs.dedup();

        println!("Missing {} state groups", missing_sgs.len());

        let map = get_missing_from_db(&conn, &missing_sgs);
        state_group_map.extend(map.into_iter());
    }

    state_group_map
}

/// Fetch the entries in state_groups_state (and their prev groups) for the
/// given `room_id` by fetching all state with the given `room_id`.
fn get_initial_data_from_db(
    conn: &Connection,
    room_id: &str,
    max_state_group: Option<i64>,
) -> BTreeMap<i64, StateGroupEntry> {
    let sql = format!(
        r#"
        SELECT m.id, prev_state_group, type, state_key, s.event_id
        FROM state_groups AS m
        LEFT JOIN state_groups_state AS s ON (m.id = s.state_group)
        LEFT JOIN state_group_edges AS e ON (m.id = e.state_group)
        WHERE m.room_id = $1 {}
    "#,
        if max_state_group.is_some() {
            "AND m.id <= $2"
        } else {
            ""
        }
    );

    let stmt = conn.prepare(&sql).unwrap();

    let trans = conn.transaction().unwrap();

    let mut rows = if let Some(s) = max_state_group {
        stmt.lazy_query(&trans, &[&room_id, &s], 1000)
    } else {
        stmt.lazy_query(&trans, &[&room_id], 1000)
    }
    .unwrap();

    let mut state_group_map: BTreeMap<i64, StateGroupEntry> = BTreeMap::new();

    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner().template("{spinner} [{elapsed}] {pos} rows retrieved"),
    );
    pb.enable_steady_tick(100);

    let mut num_rows = 0;
    while let Some(row) = rows.next().unwrap() {
        let state_group = row.get(0);

        let entry = state_group_map.entry(state_group).or_default();

        entry.prev_state_group = row.get(1);
        let etype: Option<String> = row.get(2);

        if let Some(etype) = etype {
            entry.state_map.insert(
                &etype,
                &row.get::<_, String>(3),
                row.get::<_, String>(4).into(),
            );
        }

        pb.inc(1);
        num_rows += 1;
    }

    pb.set_length(num_rows);
    pb.finish();

    state_group_map
}

/// Get any missing state groups from the database
fn get_missing_from_db(conn: &Connection, missing_sgs: &[i64]) -> BTreeMap<i64, StateGroupEntry> {
    let stmt = conn
        .prepare(
            r#"
                SELECT state_group, prev_state_group
                FROM state_group_edges
                WHERE state_group = ANY($1)
            "#,
        )
        .unwrap();
    let trans = conn.transaction().unwrap();

    let mut rows = stmt.lazy_query(&trans, &[&missing_sgs], 100).unwrap();

    // initialise the map with empty entries (the missing group may not
    // have a prev_state_group either)
    let mut state_group_map: BTreeMap<i64, StateGroupEntry> =
        missing_sgs.iter()
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
pub struct PGEscapse<'a>(pub &'a str);

impl<'a> fmt::Display for PGEscapse<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut delim = Cow::from("$$");
        while self.0.contains(&delim as &str) {
            let s: String = thread_rng().sample_iter(&Alphanumeric).take(10).collect();

            delim = format!("${}$", s).into();
        }

        write!(f, "{}{}{}", delim, self.0, delim)
    }
}

#[test]
fn test_pg_escape() {
    let s = format!("{}", PGEscapse("test"));
    assert_eq!(s, "$$test$$");

    let dodgy_string = "test$$ing";

    let s = format!("{}", PGEscapse(dodgy_string));

    // prefix and suffixes should match
    let start_pos = s.find(dodgy_string).expect("expected to find dodgy string");
    let end_pos = start_pos + dodgy_string.len();
    assert_eq!(s[..start_pos], s[end_pos..]);

    // .. and they should start and end with '$'
    assert_eq!(&s[0..1], "$");
    assert_eq!(&s[start_pos - 1..start_pos], "$");
}
