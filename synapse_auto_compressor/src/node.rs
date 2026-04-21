pub mod node {
    use crate::manager::{compress_chunks_of_database, CompressedChunkResult};
    use crate::LevelInfo;
    use napi::bindgen_prelude::*;
    use napi::{Error, Status};
    use napi_derive::napi;

    pub struct AsyncCompressor {
        db_url: String,
        chunk_size: i64,
        number_of_chunks: i64,
        default_levels: Option<String>,
    }

    #[napi]
    impl Task for AsyncCompressor {
        type Output = Vec<CompressedChunkResult>;
        type JsValue = Vec<CompressedChunkResult>;

        fn compute(&mut self) -> Result<Self::Output> {
            let levels = self
                .default_levels
                .clone()
                .unwrap_or("100,50,25".to_string());
            let levels = levels
                .parse::<LevelInfo>()
                .unwrap_or_else(|e| panic!("Error while parsing default levels: {}", e));
            let results = compress_chunks_of_database(
                &self.db_url.as_str(),
                self.chunk_size,
                &levels.0,
                self.number_of_chunks,
            )
            .map_err(|e| {
                Error::new(
                    Status::GenericFailure,
                    format!("Failure while compressing database: {}", e),
                )
            });

            Ok(results?)
        }

        fn resolve(&mut self, _: Env, output: Vec<CompressedChunkResult>) -> Result<Self::JsValue> {
            Ok(output)
        }
    }

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
    ) -> AsyncTask<AsyncCompressor> {
        AsyncTask::new(AsyncCompressor {
            db_url,
            chunk_size,
            number_of_chunks,
            default_levels,
        })
    }
}
