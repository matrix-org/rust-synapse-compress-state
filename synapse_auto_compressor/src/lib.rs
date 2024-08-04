//! This is a tool that uses the synapse_compress_state library to
//! reduce the size of the synapse state_groups_state table in a postgres
//! database.
//!
//! It adds the tables state_compressor_state and state_compressor_progress
//! to the database and uses these to enable it to incrementally work
//! on space reductions

use anyhow::Result;
#[cfg(feature = "pyo3")]
use pyo3::prelude::*;
use std::str::FromStr;

use synapse_compress_state::Level;

pub mod manager;
pub mod state_saving;

/// Helper struct for parsing the `default_levels` argument.
///
/// The compressor keeps track of a number of Levels, each of which have a maximum length,
/// current length, and an optional current head (None if level is empty, Some if a head
/// exists).
///
/// This is needed since FromStr cannot be implemented for structs
/// that aren't defined in this scope
#[derive(PartialEq, Eq, Debug, Clone)]
pub struct LevelInfo(pub Vec<Level>);

// Implement FromStr so that an argument of the form "100,50,25"
// can be used to create a vector of levels with max sizes 100, 50 and 25
// For more info see the LevelState documentation in lib.rs
impl FromStr for LevelInfo {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Stores the max sizes of each level
        let mut level_info: Vec<Level> = Vec::new();

        // Split the string up at each comma
        for size_str in s.split(',') {
            // try and convert each section into a number
            // panic if that fails
            let size: usize = size_str
                .parse()
                .map_err(|_| "Not a comma separated list of numbers")?;
            // add this parsed number to the sizes struct
            level_info.push(Level::new(size));
        }

        // Return the built up vector inside a LevelInfo struct
        Ok(LevelInfo(level_info))
    }
}

// PyO3 INTERFACE STARTS HERE
#[cfg(feature = "pyo3")]
#[pymodule]
mod synapse_auto_compressor {
    use super::*;
    use log::{error, info, LevelFilter};
    use pyo3::exceptions::PyRuntimeError;

    #[pymodule_init]
    fn init(_m: &Bound<'_, PyModule>) -> PyResult<()> {
        let _ = pyo3_log::Logger::default()
            // don't send out anything lower than a warning from other crates
            .filter(LevelFilter::Warn)
            // don't log warnings from synapse_compress_state, the
            // synapse_auto_compressor handles these situations and provides better
            // log messages
            .filter_target("synapse_compress_state".to_owned(), LevelFilter::Error)
            // log info and above for the synapse_auto_compressor
            .filter_target("synapse_auto_compressor".to_owned(), LevelFilter::Debug)
            .install();

        // ensure any panics produce error messages in the log
        log_panics::init();

        Ok(())
    }

    /// Main entry point for python code
    ///
    /// Default arguments are equivalent to using the command line tool.
    ///
    /// No defaults are provided for `db_url`, `chunk_size` and
    /// `number_of_chunks`, since these argument are mandatory.
    #[pyfunction]
    #[pyo3(signature = (
        db_url, // has no default
        chunk_size, // has no default
        number_of_chunks, // has no default
        default_levels = "100,50,25",
    ))]
    fn run_compression(
        py: Python,
        db_url: &str,
        chunk_size: i64,
        number_of_chunks: i64,
        default_levels: &str,
    ) -> PyResult<()> {
        // Announce the start of the program to the logs
        info!("synapse_auto_compressor started");

        // Parse the default_level string into a LevelInfo struct
        let default_levels = default_levels.parse::<LevelInfo>().map_err(|e| {
            PyErr::new::<PyRuntimeError, _>(format!("Unable to parse level_sizes: {}", e))
        })?;

        // Stops the compressor from holding the GIL while running
        py.allow_threads(|| {
            // call compress_chunks_of_database with the arguments supplied
            manager::compress_chunks_of_database(
                db_url,
                chunk_size,
                &default_levels.0,
                number_of_chunks,
            )
        })
        .map_err(|e| {
            error!("{}", e);
            PyErr::new::<PyRuntimeError, _>(format!("{:?}", e))
        })?;

        info!("synapse_auto_compressor finished");
        Ok(())
    }

    /// Deprecated entry point
    #[pyfunction]
    fn compress_largest_rooms(
        py: Python,
        db_url: &str,
        chunk_size: i64,
        default_levels: &str,
        number_of_chunks: i64,
    ) -> PyResult<()> {
        run_compression(py, db_url, chunk_size, number_of_chunks, default_levels)
    }
}
