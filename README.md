# Compress Synapse State Tables

This workspace contains experimental tools that attempt to reduce the number of
rows in the `state_groups_state` table inside of a Synapse Postgresql database.

# Automated tool: auto_compressor

## Introduction:

This tool is significantly more simple to use than the manual tool (described below).
It scans through all of the rows in the `state_groups` database table from the start. When
it finds a group that hasn't been compressed, it runs the compressor for a while on that
group's room, saving where it got up to. After compressing a number of these chunks it stops,
saving where it got up to for the next run of the `auto_compressor`.

It creates three extra tables in the database: `state_compressor_state` which stores the
information needed to stop and start the compressor for each room, `state_compressor_progress`
which stores the most recently compressed state group for each room and `state_compressor_total_progress`
which stores how far through the `state_groups` table the compressor has scanned.

The tool can be run manually when you are running out of space, or be scheduled to run
periodically.

## Building 

This tool requires `cargo` to be installed. See https://www.rust-lang.org/tools/install
for instructions on how to do this.

To build `auto_compressor`, clone this repository and navigate to the `autocompressor/` 
subdirectory. Then execute `cargo build`.

This will create an executable and store it in `auto_compressor/target/debug/auto_compressor`.

## Example usage
```
$ auto_compressor -p postgresql://user:pass@localhost/synapse -c 500 -n 100
```
## Running Options

- -p [POSTGRES_LOCATION] **Required**  
The configuration for connecting to the Postgres database. This should be of the form
`"postgresql://username:password@mydomain.com/database"` or a key-value pair
string: `"user=username password=password dbname=database host=mydomain.com"`
See https://docs.rs/tokio-postgres/0.7.2/tokio_postgres/config/struct.Config.html
for the full details.

- -c [CHUNK_SIZE] **Required**  
The number of state groups to work on at once. All of the entries from state_groups_state are
requested from the database for state groups that are worked on. Therefore small chunk
sizes may be needed on machines with low memory. Note: if the compressor fails to find
space savings on the chunk as a whole (which may well happen in rooms with lots of backfill
in) then the entire chunk is skipped.

- -n [CHUNKS_TO_COMPRESS] **Required**  
*CHUNKS_TO_COMPRESS* chunks of size *CHUNK_SIZE* will be compressed. The higher this
number is set to, the longer the compressor will run for.

- -d [LEVELS]  
Sizes of each new level in the compression algorithm, as a comma-separated list.
The first entry in the list is for the lowest, most granular level, with each
subsequent entry being for the next highest level. The number of entries in the
list determines the number of levels that will be used. The sum of the sizes of
the levels affects the performance of fetching the state from the database, as the
sum of the sizes is the upper bound on the number of iterations needed to fetch a
given set of state. [defaults to "100,50,25"]

## Scheduling the compressor
The automatic tool may put some strain on the database, so it might be best to schedule
it to run at a quiet time for the server. This could be done by creating an executable
script and scheduling it with something like 
[cron](https://www.man7.org/linux/man-pages/man1/crontab.1.html).

# Manual tool: synapse_compress_state

## Introduction

A manual tool that reads in the rows from `state_groups_state` and `state_group_edges` 
tables for a specified room and calculates the changes that could be made that
(hopefully) will significantly reduce the number of rows.

This tool currently *does not* write to the database by default, so should be
safe to run. If the `-o` option is specified then SQL will be written to the
given file that would change the tables to match the calculated state. (Note
that if `-t` is given then each change to a particular state group is wrapped
in a transaction). If you do wish to send the changes to the database automatically
then the `-c` flag can be set.

The SQL generated is safe to apply against the database with Synapse running. 
This is because the `state_groups` and `state_groups_state` tables are append-only:
once written to the database, they are never modified. There is therefore no danger
of a modification racing against a running Synapse. Further, this script makes its
changes within atomic transactions, and each transaction should not affect the results
from any of the queries that Synapse performs.

The tool will also ensure that the generated state deltas do give the same state
as the existing state deltas before generating any SQL.

## Building 

This tool requires `cargo` to be installed. See https://www.rust-lang.org/tools/install
for instructions on how to do this.

To build `synapse_compress_state`, clone this repository and then execute `cargo build`.

This will create an executable and store it in `target/debug/synapse_compress_state`.

## Example usage

```
$ synapse_compress_state -p "postgresql://localhost/synapse" -r '!some_room:example.com' -o out.sql -t
Fetching state from DB for room '!some_room:example.com'...
Got initial state from database. Checking for any missing state groups...
Number of state groups: 73904
Number of rows in current table: 2240043
Number of rows after compression: 165754 (7.40%)
Compression Statistics:
  Number of forced resets due to lacking prev: 34
  Number of compressed rows caused by the above: 17092
  Number of state groups changed: 2748
New state map matches old one

# It's finished, so we can now go and rewrite the DB
$ psql synapse < out.data
```

## Running Options

- -p [POSTGRES_LOCATION] **Required**  
The configuration for connecting to the Postgres database. This should be of the form
`"postgresql://username:password@mydomain.com/database"` or a key-value pair
string: `"user=username password=password dbname=database host=mydomain.com"`
See https://docs.rs/tokio-postgres/0.7.2/tokio_postgres/config/struct.Config.html
for the full details.

- -r [ROOM_ID] **Required**  
The room to process (this is the value found in the `rooms` table of the database
not the common name for the room - it should look like: "!wOlkWNmgkAZFxbTaqj:matrix.org".

- -b [MIN_STATE_GROUP]  
The state group to start processing from (non-inclusive).

- -n [GROUPS_TO_COMPRESS]  
How many groups to load into memory to compress (starting
from the 1st group in the room or the group specified by -b).

- -l [LEVELS]  
Sizes of each new level in the compression algorithm, as a comma-separated list.
The first entry in the list is for the lowest, most granular level, with each 
subsequent entry being for the next highest level. The number of entries in the
list determines the number of levels that will be used. The sum of the sizes of
the levels affects the performance of fetching the state from the database, as the
sum of the sizes is the upper bound on the number of iterations needed to fetch a
given set of state. [defaults to "100,50,25"]

- -m [COUNT]  
If the compressor cannot save this many rows from the database then it will stop early.

- -s [MAX_STATE_GROUP]  
If a max_state_group is specified then only state groups with id's lower than this
number can be compressed.

- -o [FILE]  
File to output the SQL transactions to (for later running on the database).

- -t  
If this flag is set then each change to a particular state group is wrapped in a
transaction. This should be done if you wish to apply the changes while synapse is
still running.

- -c  
If this flag is set then the changes the compressor makes will be committed to the
database. This should be safe to use while synapse is running as it wraps the changes
to every state group in it's own transaction (as if the transaction flag was set).

- -g  
If this flag is set then output the node and edge information for the state_group
directed graph built up from the predecessor state_group links. These can be looked
at in something like Gephi (https://gephi.org).


# Running tests

There are integration tests for these tools stored in `compressor_integration_tests/`.

To run the integration tests, you first need to start up a Postgres database
for the library to talk to. There is a docker-compose file that sets one up
with all of the correct tables. The tests can therefore be run as follows:

```
$ cd compressor_integration_tests/
$ docker-compose up -d
$ cargo test --workspace
$ docker-compose down
```

# Using the synapse_compress_state library

If you want to use the compressor in another project, it is recomended that you
use jemalloc `https://github.com/gnzlbg/jemallocator`. 

To prevent the progress bars from being shown, use the `no-progress-bars` feature.
(See `auto_compressor/Cargo.toml` for an example)

# Troubleshooting

## Connecting to database

### From local machine

If you setup Synapse using the instructions on https://matrix-org.github.io/synapse/latest/postgres.html
you should have a username and password to use to login to the postgres database. To run the compressor
from the machine where Postgres is running, the url will be the following:

`postgresql://synapse_user:synapse_password@localhost/synapse`

### From remote machine

If you wish to connect from a different machine, you'll need to edit your Postgres settings to allow
remote connections. This requires updating the 
[`pg_hba.conf`](https://www.postgresql.org/docs/current/auth-pg-hba-conf.html) and the `listen_addresses`
setting in [`postgresql.conf`](https://www.postgresql.org/docs/current/runtime-config-connection.html)

## Printing debugging logs

The amount of output the tools produce can be altered by setting the RUST_LOG 
environment variable to something. 

To get more logs when running the auto_compressor tool try the following:

```
$ RUST_LOG=debug auto_compressor -p postgresql://user:pass@localhost/synapse -c 50 -n 100
```

If you want to suppress all the debugging info you are getting from the 
Postgres client then try:

```
RUST_LOG=auto_compressor=debug,synapse_compress_state=debug auto_compressor [etc.]
```

This will only print the debugging information from those two packages. For more info see 
https://docs.rs/env_logger/0.9.0/env_logger/.

## Building difficulties

Building the `openssl-sys` dependency crate requires OpenSSL development tools to be installed,
and building on Linux will also require `pkg-config`

This can be done on Ubuntu  with: `$ apt-get install libssl-dev pkg-config`

Note that building requires quite a lot of memory and out-of-memory errors might not be 
obvious. It's recomended you only build these tools on machines with at least 2GB of RAM.

## Auto Compressor skips chunks when running on already compressed room

If you have used the compressor before, with certain config options, the automatic tool will
produce lots of warnings of the form: `The compressor tried to increase the number of rows in ...`

To fix this, ensure that the chunk_size is set to at least the L1 level size (so if the level
sizes are "100,50,25" then the chunk_size should be at least 100).

Note: if the level sizes being used when rerunning are different to when run previously
this might lead to less efficient compression and thus chunks being skipped, but this shouldn't
be a large problem.

## Compressor is trying to increase the number of rows

Backfilling can lead to issues with compression. The auto_compressor will
skip chunks it can't reduce the size of and so this should help jump over the backfilled 
state_groups. Lots of state resolution might also impact the ability to use the compressor.

To examine the state_group hierarchy run the manual tool on a room with the `-g` option
and look at the graphs.
