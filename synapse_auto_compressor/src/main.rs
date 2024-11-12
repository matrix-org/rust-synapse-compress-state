//! This is a tool that uses the synapse_compress_state library to
//! reduce the size of the synapse state_groups_state table in a postgres
//! database.
//!
//! It adds the tables state_compressor_state and state_compressor_progress
//! to the database and uses these to enable it to incrementally work
//! on space reductions.
//!
//! This binary calls manager::compress_chunks_of_database() with the arguments
//! provided. That is, it compresses (in batches) the top N rooms ranked by
//! amount of "uncompressed" state. This is measured by the number of rows in
//! the state_groups_state table.
//!
//! After each batch, the rows processed are marked as "compressed" (using
//! the state_compressor_progress table), and the program state is saved into
//! the state_compressor_state table so that the compressor can seemlesly
//! continue from where it left off.

#[cfg(feature = "jemalloc")]
#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

use clap::{crate_authors, crate_description, crate_name, crate_version, Arg, Command};
use log::LevelFilter;
use std::env;
use synapse_auto_compressor::{manager, state_saving, LevelInfo};

/// Execution starts here
fn main() {
    // setup the logger for the synapse_auto_compressor
    // The default can be overwritten with RUST_LOG
    // see the README for more information
    if env::var("RUST_LOG").is_err() {
        let mut log_builder = env_logger::builder();
        // Ensure panics still come through
        log_builder.filter_module("panic", LevelFilter::Error);
        // Only output errors from the synapse_compress state library
        log_builder.filter_module("synapse_compress_state", LevelFilter::Error);
        // Output log levels info and above from synapse_auto_compressor
        log_builder.filter_module("synapse_auto_compressor", LevelFilter::Info);
        log_builder.init();
    } else {
        // If RUST_LOG was set then use that
        let mut log_builder = env_logger::Builder::from_env("RUST_LOG");
        // Ensure panics still come through
        log_builder.filter_module("panic", LevelFilter::Error);
        log_builder.init();
    }
    log_panics::init();
    // Announce the start of the program to the logs
    log::info!("synapse_auto_compressor started");

    // parse the command line arguments using the clap crate
    let arguments = Command::new(crate_name!())
        .version(crate_version!())
        .author(crate_authors!("\n"))
        .about(crate_description!())
        .arg(
            Arg::new("postgres-url")
                .short('p')
                .value_name("POSTGRES_LOCATION")
                .help("The configruation for connecting to the postgres database.")
                .long_help(concat!(
                    "The configuration for connecting to the Postgres database. This should be of the form ",
                    r#""postgresql://username:password@mydomain.com/database" or a key-value pair "#,
                    r#"string: "user=username password=password dbname=database host=mydomain.com" "#,
                    "See https://docs.rs/tokio-postgres/0.7.2/tokio_postgres/config/struct.Config.html ",
                    "for the full details."
                ))
                .num_args(1)
                .required(true),
        ).arg(
            Arg::new("chunk_size")
                .short('c')
                .value_name("COUNT")
                .value_parser(clap::value_parser!(i64))
                .help("The maximum number of state groups to load into memroy at once")
                .long_help(concat!(
                    "The number of state_groups to work on at once. All of the entries",
                    " from state_groups_state are requested from the database",
                    " for state groups that are worked on. Therefore small",
                    " chunk sizes may be needed on machines with low memory.",
                    " (Note: if the compressor fails to find space savings on the",
                    " chunk as a whole (which may well happen in rooms with lots",
                    " of backfill in) then the entire chunk is skipped.)",
                ))
                .num_args(1)
                .required(true),
        ).arg(
            Arg::new("default_levels")
                .short('l')
                .value_name("LEVELS")
                .value_parser(clap::value_parser!(LevelInfo))
                .help("Sizes of each new level in the compression algorithm, as a comma separated list.")
                .long_help(concat!(
                    "Sizes of each new level in the compression algorithm, as a comma separated list.",
                    " The first entry in the list is for the lowest, most granular level,",
                    " with each subsequent entry being for the next highest level.",
                    " The number of entries in the list determines the number of levels",
                    " that will be used.",
                    "\nThe sum of the sizes of the levels effect the performance of fetching the state",
                    " from the database, as the sum of the sizes is the upper bound on number of",
                    " iterations needed to fetch a given set of state.",
                ))
                .default_value("100,50,25")
                .num_args(1)
                .required(false),
        ).arg(
            Arg::new("number_of_chunks")
                .short('n')
                .value_name("CHUNKS_TO_COMPRESS")
                .value_parser(clap::value_parser!(i64))
                .help("The number of chunks to compress")
                .long_help(concat!(
                    "This many chunks of the database will be compressed. The higher this number is set to, ",
                    "the longer the compressor will run for."
                ))
                .num_args(1)
                .required(true),
        ).get_matches();

    // The URL of the database
    let db_url = arguments
        .get_one::<String>("postgres-url")
        .expect("A database url is required");

    // The number of state groups to work on at once
    let chunk_size = arguments
        .get_one("chunk_size")
        .copied()
        .expect("A chunk size is required");

    // The default structure to use when compressing
    let default_levels = arguments
        .get_one::<LevelInfo>("default_levels")
        .cloned()
        .unwrap();

    // The number of rooms to compress with this tool
    let number_of_chunks = arguments
        .get_one("number_of_chunks")
        .copied()
        .expect("number_of_chunks is required");

    // Connect to the database and create the 2 tables this tool needs
    // (Note: if they already exist then this does nothing)
    let mut client = state_saving::connect_to_database(db_url)
        .unwrap_or_else(|e| panic!("Error occured while connecting to {}: {}", db_url, e));
    state_saving::create_tables_if_needed(&mut client)
        .unwrap_or_else(|e| panic!("Error occured while creating tables in database: {}", e));

    // call compress_chunks_of_database with the arguments supplied
    // panic if an error is produced
    manager::compress_chunks_of_database(db_url, chunk_size, &default_levels.0, number_of_chunks)
        .unwrap();

    log::info!("synapse_auto_compressor finished");
}
