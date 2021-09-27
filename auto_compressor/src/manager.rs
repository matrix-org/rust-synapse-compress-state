// This module contains functions that carry out diffferent types
// of compression on the database.

use crate::state_saving::{
    connect_to_database, create_tables_if_needed, get_next_room_to_compress,
    read_room_compressor_state, write_room_compressor_state,
};
use anyhow::{bail, Context, Result};
use log::{debug, info, warn};
use synapse_compress_state::{continue_run, ChunkStats, Level};

/// Runs the compressor on a chunk of the room
///
/// Returns `Some(chunk_stats)` if the compressor has progressed
/// and `None` if it had already got to the end of the room
///
/// # Arguments
///
/// * `db_url`          -   The URL of the postgres database that synapse is using.
///                         e.g. "postgresql://user:password@domain.com/synapse"
///
/// * `room_id`         -   The id of the room to run the compressor on. Note this
///                         is the id as stored in the database and will look like
///                         "!aasdfasdfafdsdsa:matrix.org" instead of the common
///                         name
///
/// * `chunk_size`      -   The number of state_groups to work on. All of the entries
///                         from state_groups_state are requested from the database
///                         for state groups that are worked on. Therefore small
///                         chunk sizes may be needed on machines with low memory.
///                         (Note: if the compressor fails to find space savings on the
///                         chunk as a whole (which may well happen in rooms with lots
///                         of backfill in) then the entire chunk is skipped.)
///
/// * `default_levels`  -   If the compressor has never been run on this room before
///                         then we need to provide the compressor with some information
///                         on what sort of compression structure we want. The default that
///                         the library suggests is `vec![Level::new(100), Level::new(50), Level::new(25)]`
pub fn run_compressor_on_room_chunk(
    db_url: &str,
    room_id: &str,
    chunk_size: i64,
    default_levels: &[Level],
) -> Result<Option<ChunkStats>> {
    // connect to the database
    let mut client =
        connect_to_database(db_url).with_context(|| format!("Failed to connect to {}", db_url))?;

    // Access the database to find out where the compressor last got up to
    let retrieved_state = read_room_compressor_state(&mut client, room_id)
        .with_context(|| format!("Failed to read compressor state for room {}", room_id,))?;

    // If the database didn't contain any information, then use the default state
    let (start, level_info) = match retrieved_state {
        Some((s, l)) => (Some(s), l),
        None => (None, default_levels.to_vec()),
    };

    // run the compressor on this chunk
    let option_chunk_stats = continue_run(start, chunk_size, db_url, room_id, &level_info);

    if option_chunk_stats.is_none() {
        debug!("No work to do on this room...");
        return Ok(None);
    }

    // Ok to unwrap because have checked that it's not None
    let chunk_stats = option_chunk_stats.unwrap();

    debug!("{:?}", chunk_stats);

    // Check to see whether the compressor sent its changes to the database
    if !chunk_stats.commited {
        if chunk_stats.new_num_rows - chunk_stats.original_num_rows != 0 {
            warn!(
                "The compressor tried to increase the number of rows in {} between {:?} and {}. Skipping...",
                room_id, start, chunk_stats.last_compressed_group,
            );
        }

        // Skip over the failed chunk and set the level info to the default (empty) state
        write_room_compressor_state(
            &mut client,
            room_id,
            default_levels,
            chunk_stats.last_compressed_group,
        )
        .with_context(|| {
            format!(
                "Failed to skip chunk in room {} between {:?} and {}",
                room_id, start, chunk_stats.last_compressed_group
            )
        })?;

        return Ok(Some(chunk_stats));
    }

    // Save where we got up to after this successful commit
    write_room_compressor_state(
        &mut client,
        room_id,
        &chunk_stats.new_level_info,
        chunk_stats.last_compressed_group,
    )
    .with_context(|| {
        format!(
            "Failed to save state after compressing chunk in room {} between {:?} and {}",
            room_id, start, chunk_stats.last_compressed_group
        )
    })?;

    Ok(Some(chunk_stats))
}

/// Runs the compressor in chunks on rooms with the lowest uncompressed state group ids
///
/// # Arguments
///
/// * `db_url`          -   The URL of the postgres database that synapse is using.
///                         e.g. "postgresql://user:password@domain.com/synapse"
///
/// * `chunk_size`      -   The number of state_groups to work on. All of the entries
///                         from state_groups_state are requested from the database
///                         for state groups that are worked on. Therefore small
///                         chunk sizes may be needed on machines with low memory.
///                         (Note: if the compressor fails to find space savings on the
///                         chunk as a whole (which may well happen in rooms with lots
///                         of backfill in) then the entire chunk is skipped.)
///
/// * `default_levels`  -   If the compressor has never been run on this room before
///                         Then we need to provide the compressor with some information
///                         on what sort of compression structure we want. The default that
///                         the library suggests is empty levels with max sizes of 100, 50 and 25
///
/// * `number_of_chunks`-   The number of chunks to compress. The larger this number is, the longer
///                         the compressor will run for.
pub fn compress_chunks_of_database(
    db_url: &str,
    chunk_size: i64,
    default_levels: &[Level],
    number_of_chunks: i64,
) -> Result<()> {
    // connect to the database
    let mut client = connect_to_database(db_url)
        .with_context(|| format!("Failed to connect to database at {}", db_url))?;

    create_tables_if_needed(&mut client).context("Failed to create state compressor tables")?;

    let mut skipped_chunks = 0;
    let mut rows_saved = 0;
    let mut chunks_processed = 0;

    while chunks_processed < number_of_chunks {
        let room_to_compress = get_next_room_to_compress(&mut client)
            .context("Failed to work out what room to compress next")?;

        if room_to_compress.is_none() {
            break;
        }

        let room_to_compress =
            room_to_compress.expect("Have checked that rooms_to_compress is not None");

        info!(
            "Running compressor on room {} with chunk size {}",
            room_to_compress, chunk_size
        );

        let work_done =
            run_compressor_on_room_chunk(db_url, &room_to_compress, chunk_size, default_levels)?;

        if let Some(ref chunk_stats) = work_done {
            if chunk_stats.commited {
                let savings = chunk_stats.original_num_rows - chunk_stats.new_num_rows;
                rows_saved += chunk_stats.original_num_rows - chunk_stats.new_num_rows;
                debug!("Saved {} rows for room {}", savings, room_to_compress);
            } else {
                skipped_chunks += 1;
                debug!(
                    "Unable to make savings for room {}, skipping chunk",
                    room_to_compress
                );
            }
            chunks_processed += 1;
        } else {
            bail!("Ran the compressor on a room that had no more work to do!")
        }
    }
    info!(
        "Finished running compressor. Saved {} rows. Skipped {}/{} chunks",
        rows_saved, skipped_chunks, chunks_processed
    );
    Ok(())
}
