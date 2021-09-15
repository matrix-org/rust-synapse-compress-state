// This module contains functions to communicate with the database

use anyhow::{bail, Result};
use core::fmt;
use rand::{distributions::Alphanumeric, thread_rng, Rng};
use std::borrow::Cow;
use synapse_compress_state::Level;

use openssl::ssl::{SslConnector, SslMethod, SslVerifyMode};
use postgres::{fallible_iterator::FallibleIterator, Client};
use postgres_openssl::MakeTlsConnector;

/// Connects to the database and returns a postgres client
///
/// # Arguments
///
/// * `db_url`          -   The URL of the postgres database that synapse is using.
///                         e.g. "postgresql://user:password@domain.com/synapse"
pub fn connect_to_database(db_url: &str) -> Result<Client> {
    let mut builder = SslConnector::builder(SslMethod::tls())?;
    builder.set_verify(SslVerifyMode::NONE);
    let connector = MakeTlsConnector::new(builder.build());

    let client = Client::connect(db_url, connector)?;
    Ok(client)
}

/// Creates the state_compressor_state and state_compressor progress tables
///
/// If these tables already exist then this function does nothing
///
/// # Arguments
///
/// * `client`        - A postgres client used to send the requests to the database
pub fn create_tables_if_needed(client: &mut Client) -> Result<()> {
    let create_state_table = r#"
        CREATE TABLE IF NOT EXISTS state_compressor_state (
            room_id TEXT NOT NULL,
            level_num INT NOT NULL,
            max_size INT NOT NULL,
            current_length INT NOT NULL,
            current_head BIGINT,
            UNIQUE (room_id, level_num)
        )"#;

    client.execute(create_state_table, &[])?;

    let create_state_table_indexes = r#"
        CREATE INDEX IF NOT EXISTS state_compressor_state_index ON state_compressor_state (room_id)"#;

    client.execute(create_state_table_indexes, &[])?;

    let create_progress_table = r#"
        CREATE TABLE IF NOT EXISTS state_compressor_progress (
            room_id TEXT PRIMARY KEY,
            last_compressed BIGINT NOT NULL
        )"#;

    client.execute(create_progress_table, &[])?;

    Ok(())
}

/// Retrieve the level info so we can restart the compressor
///
/// # Arguments
///
/// * `client`        - A postgres client used to send the requests to the database
/// * `room_id`       - The room who's saved compressor state we want to load
pub fn read_room_compressor_state(
    client: &mut Client,
    room_id: &str,
) -> Result<Option<(i64, Vec<Level>)>> {
    // Query to retrieve all levels from state_compressor_state
    // Ordered by ascending level_number
    let sql = r#"
        SELECT level_num, max_size, current_length, current_head, last_compressed
        FROM state_compressor_state 
        JOIN state_compressor_progress USING (room_id)
        WHERE room_id = $1
        ORDER BY level_num ASC
    "#;

    // send the query to the database
    let mut levels = client.query_raw(sql, &[room_id])?;

    // Needed to ensure that the rows are for unique consecutive levels
    // starting from 1 (i.e of form [1,2,3] not [0,1,2] or [1,1,2,2,3])
    let mut prev_seen = 0;

    // The vector to store the level info from the database in
    let mut level_info: Vec<Level> = Vec::new();
    let mut last_compressed: i64 = 0;

    // Loop through all the rows retrieved by that query
    while let Some(l) = levels.next()? {
        // Read out the fields into variables
        //
        // Some of these are `usize` as they may be used to index vectors, but stored as Postgres
        // type `INT` which is the same as`i32`.
        //
        // Since usize is unlikely to be ess than 32 bits wide, this conversion should be safe
        let level_num: usize = l.get::<_, i32>("level_num") as usize;
        let max_size: usize = l.get::<_, i32>("max_size") as usize;
        let current_length: usize = l.get::<_, i32>("current_length") as usize;
        let current_head: Option<i64> = l.get("current_head");
        last_compressed = l.get::<_, i64>("last_compressed"); // possibly rewrite same value

        // Check that there aren't multiple entries for the same level number
        // in the database. (Should be impossible due to unique key constraint)
        if prev_seen == level_num {
            bail!(
                "The level {} occurs twice in state_compressor_state for room {}",
                level_num,
                room_id,
            );
        }

        // Check that there is no missing level in the database
        // e.g. if the previous row retrieved was for level 1 and this
        // row is for level 3 then since the SQL query orders the results
        // in ascenting level numbers, there was no level 2 found!
        if prev_seen != level_num - 1 {
            bail!("Levels between {} and {} are missing", prev_seen, level_num,);
        }

        // if the level is not empty, then it must have a head!
        if current_head.is_none() && current_length != 0 {
            bail!(
                "Level {} has no head but current length is {} in room {}",
                level_num,
                current_length,
                room_id,
            );
        }

        // If the level has more groups in than the maximum then something is wrong!
        if current_length > max_size {
            bail!(
                "Level {} has length {} but max size {} in room {}",
                level_num,
                current_length,
                max_size,
                room_id,
            );
        }

        // Add this level to the level_info vector
        level_info.push(Level::restore(max_size, current_length, current_head));
        // Mark the previous level_number seen as the current one
        prev_seen = level_num;
    }

    // If we didn't retrieve anything from the database then there is no saved state
    // in the database!
    if level_info.is_empty() {
        return Ok(None);
    }

    // Return the compressor state we retrieved
    Ok(Some((last_compressed, level_info)))
}

/// Save the level info so it can be loaded by the next run of the compressor
///
/// # Arguments
///
/// * `client`            - A postgres client used to send the requests to the database
/// * `room_id`           - The room who's saved compressor state we want to save
/// * `level_info`        - The state that can be used to restore the compressor later
/// * `last_compressed`   - The last state_group that was compressed. This is needed
///                         so that the compressor knows where to start from next
pub fn write_room_compressor_state(
    client: &mut Client,
    room_id: &str,
    level_info: &[Level],
    last_compressed: i64,
) -> Result<()> {
    // The query we are going to build up
    let mut sql = String::new();

    // Go through every level that the compressor is using
    for (level_num, level) in level_info.iter().enumerate() {
        // the 1st level is level 1 not level 0, but enumerate starts at 0
        // so need to add 1 to get correct number
        let level_num = level_num + 1;

        // bring the level info out of the Level struct
        let (max_size, current_len, current_head) = (
            level.get_max_length(),
            level.get_current_length(),
            level.get_head(),
        );

        // Current_head is either a value or NULL
        // need to convert from Option so that this can be placed into a string
        let current_head = match current_head {
            Some(s) => s.to_string(),
            None => "NULL".to_string(),
        };

        // Update the database with this compressor state information
        //
        // Some of these are `usize` as they may be used to index vectors, but stored as Postgres
        // type `INT` which is the same as`i32`.
        //
        // Since these values should always be small, this conversion should be safe.
        sql.push_str(&format!(
            r#"
            INSERT INTO state_compressor_state 
                (room_id, level_num, max_size, current_length, current_head) 
                VALUES ({0}, {1}, {2}, {3}, {4})
            ON CONFLICT (room_id, level_num) 
                DO UPDATE SET (max_size, current_length, current_head) 
                    = (excluded.max_size, excluded.current_length, excluded.current_head);
            "#,
            PGEscape(room_id),
            level_num as i32,
            max_size as i32,
            current_len as i32,
            current_head,
        ));
    }

    // Update the database with this progress information
    sql.push_str(&format!(
        r#"
            INSERT INTO state_compressor_progress (room_id, last_compressed) 
                VALUES ({0},{1})
            ON CONFLICT (room_id)
                DO UPDATE SET last_compressed = excluded.last_compressed;
        "#,
        PGEscape(room_id),
        last_compressed,
    ));

    // Wrap all the changes to the state for this room in a transaction
    // This prevents accidentally having malformed compressor start info
    let mut write_transaction = client.transaction()?;
    write_transaction.batch_execute(&sql)?;
    write_transaction.commit()?;

    Ok(())
}

// TODO: find a library that has an existing safe postgres escape function
/// Helper function that escapes the wrapped text when writing SQL
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
