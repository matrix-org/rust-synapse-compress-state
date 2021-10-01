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

//! This is a tool that attempts to further compress state maps within a
//! Synapse instance's database. Specifically, it aims to reduce the number of
//! rows that a given room takes up in the `state_groups_state` table.

#[cfg(feature = "jemalloc")]
#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

use log::LevelFilter;
use std::env;
use std::io::Write;

use synapse_compress_state as comp_state;

fn main() {
    // setup the logger
    // The default can be overwritten with RUST_LOG
    // see the README for more information
    if env::var("RUST_LOG").is_err() {
        let mut log_builder = env_logger::builder();
        // Only output the log message (and not the prefixed timestamp etc.)
        log_builder.format(|buf, record| writeln!(buf, "{}", record.args()));
        // By default print all of the debugging messages from this library
        log_builder.filter_module("synapse_compress_state", LevelFilter::Debug);
        log_builder.init();
    } else {
        // If RUST_LOG was set then use that
        env_logger::Builder::from_env("RUST_LOG").init();
    }

    comp_state::run(comp_state::Config::parse_arguments());
}
