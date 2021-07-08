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

use synapse_compress_state as comp_state;

fn main() {
    comp_state::run(
        comp_state::Config::parse_arguments());
}