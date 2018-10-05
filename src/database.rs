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

use std::collections::BTreeMap;

use StateGroupEntry;

/// Fetch the entries in state_groups_state (and their prev groups) for the
/// given `room_id` by connecting to the postgres database at `db_url`.
pub fn get_data_from_db(db_url: &str, room_id: &str) -> BTreeMap<i64, StateGroupEntry> {
    let conn = Connection::connect(db_url, TlsMode::None).unwrap();

    let mut state_group_map = get_initial_data_from_db(&conn, room_id);

    println!("Got initial state from database. Checking for any missing state groups...");

    // Due to reasons some of the state groups appear in the edges table, but
    // not in the state_groups_state table. This means they don't get included
    // in our DB queries, so we have to fetch any missing groups explicitly.
    // Since the returned groups may themselves reference groups we don't have,
    // we need to do this recursively until we don't find any more missing.
    loop {
        let missing_sgs: Vec<_> = state_group_map
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
            }).collect();

        if missing_sgs.is_empty() {
            break;
        }

        println!("Missing {} state groups", missing_sgs.len());

        let map = get_missing_from_db(&conn, &missing_sgs);
        state_group_map.extend(map.into_iter());
    }

    state_group_map
}

/// Fetch the entries in state_groups_state (and their prev groups) for the
/// given `room_id` by fetching all state with the given `room_id`.
fn get_initial_data_from_db(conn: &Connection, room_id: &str) -> BTreeMap<i64, StateGroupEntry> {
    let stmt = conn
        .prepare(
            r#"
                SELECT m.id, prev_state_group, type, state_key, s.event_id
                FROM state_groups AS m
                LEFT JOIN state_groups_state AS s ON (m.id = s.state_group)
                LEFT JOIN state_group_edges AS e ON (m.id = e.state_group)
                WHERE m.room_id = $1
            "#,
        ).unwrap();

    let trans = conn.transaction().unwrap();
    let mut rows = stmt.lazy_query(&trans, &[&room_id], 1000).unwrap();

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
        ).unwrap();
    let trans = conn.transaction().unwrap();

    let mut rows = stmt.lazy_query(&trans, &[&missing_sgs], 100).unwrap();

    let mut state_group_map: BTreeMap<i64, StateGroupEntry> = BTreeMap::new();

    while let Some(row) = rows.next().unwrap() {
        let state_group = row.get(0);

        let entry = state_group_map.entry(state_group).or_default();

        entry.prev_state_group = row.get(1);
    }

    state_group_map
}
