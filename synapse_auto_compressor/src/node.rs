#[cfg(feature = "node")]
pub mod node {
    use crate::manager::compress_chunks_of_database;
    use crate::LevelInfo;
    use napi::bindgen_prelude::*;
    use napi_derive::napi;

    #[napi(constructor)]
    pub struct CompressedChunkResult {
        pub room_id: String,
        pub original_num_rows: i32,
        pub new_num_rows: i32,
        pub skipped: bool,
    }

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
            let chunk_results = match compress_chunks_of_database(
                &self.db_url.as_str(),
                self.chunk_size,
                &levels.0,
                self.number_of_chunks,
            ) {
                Ok(val) => val,
                Err(e) => {
                    return Err(Error::new(
                        Status::GenericFailure,
                        format!("Failure while compressing database: {}", e),
                    ))
                }
            };

            let mut results = vec![];
            for result in chunk_results.iter() {
                results.push(CompressedChunkResult {
                    room_id: result.room_id.clone(),
                    original_num_rows: result.original_num_rows.clone(),
                    new_num_rows: result.new_num_rows.clone(),
                    skipped: result.skipped.clone(),
                });
            }
            Ok(results)
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
