pub mod node {
    use crate::LevelInfo;
    use crate::manager::{CompressedChunkResult,compress_chunks_of_database};
    use napi::{Error, Status};
    use napi_derive::napi;

    /// Main entry point for nodejs code
    ///
    /// Default arguments are equivalent to using the command line tool.
    ///
    /// No defaults are provided for `db_url`, `chunk_size` and
    /// `number_of_chunks`, since these argument are mandatory.
    #[napi]
    pub fn run_compression(
        db_url: String,
        chunk_size: i64,
        number_of_chunks: i64,
        default_levels: Option<String>,
    ) -> Result<Vec<CompressedChunkResult>, Error> {
        let levels = default_levels.unwrap_or("100,50,25".to_string());
        let levels = levels.parse::<LevelInfo>().unwrap_or_else(|e| {
            panic!("Error while parsing default levels: {}", e)
        });
        let results = compress_chunks_of_database(
            &db_url.as_str(),
            chunk_size,
            &levels.0,
            number_of_chunks,
        ).map_err(|e| Error::new(Status::GenericFailure, format!("Failure while compressing database: {}", e)));

        Ok(results?)
    }
}