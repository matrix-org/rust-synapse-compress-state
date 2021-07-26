use openssl::ssl::{SslConnector, SslMethod, SslVerifyMode};
use postgres::{fallible_iterator::FallibleIterator, Client, Error};
use postgres_openssl::MakeTlsConnector;

// Connects to the database and returns the client to use for the rest of the program
pub fn connect_to_database(db_url: &str) -> Result<Client, Error> {
    let mut builder = SslConnector::builder(SslMethod::tls()).unwrap();
    builder.set_verify(SslVerifyMode::NONE);
    let connector = MakeTlsConnector::new(builder.build());

    Client::connect(db_url, connector)
}

// Creates the state_compressor_state table in the database if it doesn't already
// exist
pub fn create_tables_if_needed(client: &mut Client) -> Result<u64, Error> {
    let sql = r#"
        CREATE TABLE IF NOT EXISTS state_compressor_state (
            room_id TEXT NOT NULL,
            level_num INT NOT NULL,
            max_size INT NOT NULL,
            current_length INT NOT NULL,
            current_head BIGINT NOT NULL
        )"#;

    client.execute(sql, &[])
}

// Helper type to store level info in
// fields are max length, current length and current head
type LevelInfo = Vec<(usize, usize, Option<i64>)>;

// Retrieve the level info so we can restart the compressor on
// a given room
pub fn read_room_compressor_state(
    client: &mut Client,
    room_id: &str,
) -> Result<Option<LevelInfo>, Error> {
    // Query to retrieve all levels from state_compressor_state
    // Ordered by ascending level_number
    let sql = r#"
        SELECT level_num, max_size, current_length, current_head
        FROM state_compressor_state as s
        WHERE s.room_id = $1
        ORDER BY level_num ASC
    "#;

    // send the query to the database
    let mut levels = client.query_raw(sql, &[room_id])?;

    // Needed to ensure that the rows are for unique consecutive levels
    // starting from 1 (i.e of form [1,2,3] not [0,1,2] or [1,1,2,2,3])
    let mut prev_seen = 0;

    // The vector to store the level info from the database in
    let mut level_info: LevelInfo = Vec::new();

    // Loop through all the rows retrieved by that query
    while let Some(l) = levels.next()? {
        // read out the fields into variables
        let level_num: usize = l.get::<_, i64>(0) as usize;
        let max_size: usize = l.get::<_, i64>(1) as usize;
        let current_length: usize = l.get::<_, i64>(2) as usize;

        // Note that the database stores an int but we want an Option.
        // Since no state_group 0 is created by synapse we use 0 to
        // represent None. Note that even if this isn't met, we only
        // compresss groups with ID above 0 (so worst case is that
        // group 0 is not compressed which is fine)
        let current_head: Option<i64> = match l.get::<_, i64>(3) {
            0 => None,
            n => Some(n),
        };

        // Check that there aren't multiple entries for the same level number
        // in the database.
        if prev_seen == level_num {
            panic!(
                "The level {} occurs twice in state_compressor_state for room {}",
                level_num, room_id,
            );
        }

        // Check that there is no missing level in the database
        // e.g. if the previous row retrieved was for level 1 and this
        // row is for level 3 then since the SQL query orders the results
        // in ascenting level numbers, there was no level 2 found!
        if prev_seen != level_num - 1 {
            panic!("Levels between {} and {} are missing", prev_seen, level_num,);
        }

        // if the level is not empty, then it must have a head!
        if current_head.is_none() && current_length != 0 {
            panic!(
                "Level {} has no head but current length is {} in room {}",
                level_num, current_length, room_id,
            )
        }

        // If the level has more groups in than the maximum then something is wrong!
        if current_length > max_size {
            panic!(
                "Level {} has length {} but max size {} in room {}",
                level_num, current_length, max_size, room_id,
            );
        }

        // Add this level to the level_info vector
        level_info.push((max_size, current_length, current_head));
        // Mark the previous level_number seen as the current one
        prev_seen = level_num;
    }

    // If we didn't retrieve anything from the database then there is no saved state
    // in the database!
    if level_info.is_empty() {
        return Ok(None);
    }

    // Return the compressor state we retrieved
    Ok(Some(level_info))
}
