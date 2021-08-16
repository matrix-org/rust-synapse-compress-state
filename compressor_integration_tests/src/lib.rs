use openssl::ssl::{SslConnector, SslMethod, SslVerifyMode};
use postgres::Client;
use postgres_openssl::MakeTlsConnector;
use rand::{distributions::Alphanumeric, thread_rng, Rng};
use std::{borrow::Cow, collections::BTreeMap, fmt};

use synapse_compress_state::StateGroupEntry;

pub mod map_builder;

pub static DB_URL: &str = "postgresql://synapse_user:synapse_pass@localhost/synapse";

pub fn add_contents_to_database(room_id: &str, state_group_map: &BTreeMap<i64, StateGroupEntry>) {
    // connect to the database
    let mut builder = SslConnector::builder(SslMethod::tls()).unwrap();
    builder.set_verify(SslVerifyMode::NONE);
    let connector = MakeTlsConnector::new(builder.build());

    let mut client = Client::connect(DB_URL, connector).unwrap();

    // build up the query
    let mut sql = "".to_string();

    for (sg, entry) in state_group_map {
        // create the entry for state_groups
        sql.push_str(&format!(
            "INSERT INTO state_groups (id, room_id, event_id) VALUES ({},{},{});\n",
            sg,
            PGEscape(room_id),
            PGEscape("left_blank")
        ));

        // create the entry in state_group_edges IF exists
        if let Some(prev_sg) = entry.prev_state_group {
            sql.push_str(&format!(
                "INSERT INTO state_group_edges (state_group, prev_state_group) VALUES ({}, {});\n",
                sg, prev_sg
            ));
        }

        // write entry for each row in delta
        if !entry.state_map.is_empty() {
            sql.push_str("INSERT INTO state_groups_state (state_group, room_id, type, state_key, event_id) VALUES");

            let mut first = true;
            for ((t, s), e) in entry.state_map.iter() {
                if first {
                    sql.push_str("     ");
                    first = false;
                } else {
                    sql.push_str("    ,");
                }
                sql.push_str(&format!(
                    "({}, {}, {}, {}, {})",
                    sg,
                    PGEscape(room_id),
                    PGEscape(t),
                    PGEscape(s),
                    PGEscape(e)
                ));
            }
            sql.push_str(";\n");
        }
    }

    client.batch_execute(&sql).unwrap();
}

pub fn empty_database() {
    // connect to the database
    let mut builder = SslConnector::builder(SslMethod::tls()).unwrap();
    builder.set_verify(SslVerifyMode::NONE);
    let connector = MakeTlsConnector::new(builder.build());

    let mut client = Client::connect(DB_URL, connector).unwrap();

    // delete all the contents from all three tables
    let mut sql = "".to_string();
    sql.push_str("DELETE FROM state_groups;\n");
    sql.push_str("DELETE FROM state_group_edges;\n");
    sql.push_str("DELETE FROM state_groups_state;\n");

    client.batch_execute(&sql).unwrap();
}

struct PGEscape<'a>(pub &'a str);

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
