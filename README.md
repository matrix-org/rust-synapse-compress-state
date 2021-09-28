# Compress Synapse State Tables

This workspace contains experimental tools that attempt to reduce the number of
rows in the `state_groups_state` table inside of a Synapse Postgresql database.

# Introduction to the state tables and compression

## What is state?
State is things like who is in a room, what the room topic/name is, who has
what privilege levels etc. Synapse keeps track of it so that it can spot invalid
events (e.g. ones sent by banned users, or by people with insufficient privilege).

## What is a state group?

Synapse needs to keep track of the state at the moment of each event. A state group
corresponds to a unique state. The database table `event_to_state_groups` keeps track
of the mapping from event ids to state group ids.

Consider the following simplified example:
```
State group id   |          State
_____________________________________________
       1         |      Alice in room
       2         | Alice in room, Bob in room
       3         |        Bob in room


Event id |     What the event was
______________________________________
    1    |    Alice sends a message
    3    |     Bob joins the room
    4    |     Bob sends a message
    5    |    Alice leaves the room
    6    |     Bob sends a message


Event id | State group id
_________________________
    1    |       1
    2    |       1
    3    |       2
    4    |       2
    5    |       3
    6    |       3
```
## What are deltas and predecessors?
When a new state event happens (e.g. Bob joins the room) a new state group is created.
BUT instead of copying all of the state from the previous state group, we just store
the change from the previous group (saving on lots of storage space!). The difference
from the previous state group is called the "delta".

So for the previous example, we would have the following (Note only rows 1 and 2 will
make sense at this point):

```
State group id | Previous state group id |      Delta
____________________________________________________________
       1       |          NONE           |   Alice in room
       2       |           1             |    Bob in room
       3       |          NONE           |    Bob in room
```
So why is state group 3's previous state group NONE and not 2? Well, the way that deltas
work in Synapse is that they can only add in new state or overwrite old state, but they
cannot remove it. (So if the room topic is changed then that is just overwriting state,
but removing Alice from the room is neither an addition nor an overwriting). If it is
impossible to find a delta, then you just start from scratch again with a "snapshot" of
the entire state. 

(NOTE this is not documentation on how synapse handles leaving rooms but is purely for illustrative
purposes)

The state of a state group is worked out by following the previous state group's and adding
together all of the deltas (with the most recent taking precedence).

The mapping from state group to previous state group takes place in `state_group_edges`
and the deltas are stored in `state_groups_state`.

## What are we compressing then?
In order to speed up the conversion from state group id to state, there is a limit of 100 
hops set by synapse (that is: we will only ever have to look up the deltas for a maximum of 
100 state groups). It does this by taking another "snapshot" every 100 state groups.

However, it is these snapshots that take up the bulk of the storage in a synapse database,
so we want to find a way to reduce the number of them without dramatically increasing the
maximum number of hops needed to do lookups.


## Compression Algorithm

The algorithm works by attempting to create a *tree* of deltas, produced by
appending state groups to different "levels". Each level has a maximum size, where
each state group is appended to the lowest level that is not full. This tool calls a 
state group "compressed" once it has been added to
one of these levels.

This produces a graph that looks approximately like the following, in the case
of having two levels with the bottom level (L1) having a maximum size of 3:

```
L2 <-------------------- L2 <---------- ...
^--- L1 <--- L1 <--- L1  ^--- L1 <--- L1 <--- L1

NOTE: A <--- B means that state group B's predecessor is A
```
The structure that synapse creates by default would be equivalent to having one level with
a maximum length of 100. 

**Note**: Increasing the sum of the sizes of levels will increase the time it
takes to query the full state of a given state group.

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

The output from the auto_compressor will be sent to `auto_compressor.log` (in the directory
that the compressor is run from).

## Building 

This tool requires `cargo` to be installed. See https://www.rust-lang.org/tools/install
for instructions on how to do this.

To build `auto_compressor`, clone this repository and navigate to the `autocompressor/` 
subdirectory. Then execute `cargo build`.

This will create an executable and store it in `auto_compressor/target/debug/auto_compressor`.

## Example usage
```
$ auto_compressor -p postgresql://user:pass@localhost/synapse -c 500 -l '100,50,25' -n 100
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
*CHUNKS_TO_COMPRESS* chunks of size *CHUNK_SIZE* will be compressed.

- -d [LEVELS]  
Sizes of each new level in the compression algorithm, as a comma-separated list.
The first entry in the list is for the lowest, most granular level, with each
subsequent entry being for the next highest level. The number of entries in the
list determines the number of levels that will be used. The sum of the sizes of
the levels affects the performance of fetching the state from the database, as the
sum of the sizes is the upper bound on the number of iterations needed to fetch a
given set of state. [defaults to "100,50,25"]

## Scheduling the compressor
Create the following script and save it somewhere sensible
(e.g. `/home/synapse/compress.sh`)

```
#!/bin/bash

cd /home/synapse/rust-synapse-compress-state/

URL=postgresql://user::pass@domain.com/synapse
CHUNK_SIZE=500
LEVELS="100,50,25"
NUMBER_OF_CHUNKS=100

/home/synapse/rust-synapse-compress-state/target/debug/auto_compressor \
-p $URL \
-c $CHUNK_SIZE \
-l $LEVELS \
-n $NUMBER_OF_ROOMS
```

Make it executable with `chmod +x compress.sh`

Then run `crontab -e` to edit your scheduled tasks and add the following:

```
# Run every day at 3:00am
00 3 * * * /home/synapse/compress.sh
```

## Using as a python library

The compressor can also be built into a python library as it uses PyO3. It can be
built and installed into the current virtual environment by running `maturin develop`:

1. Create a virtual environment in the place you want to use the compressor from
(if it doesn't already exist)  
`$ virtualenv -p python3 venv`

2. Activate the virtual environment  
`$ source venv/bin/activate`

3. Build and install the library  
`$ cd /home/synapse/rust-synapse-compress-state/auto_compressor`  
`$ pip install maturin`  
`$ maturin develop`

The following code does exactly the same as the command-line example from above:

```python
import auto_compressor as comp

comp.compress_largest_rooms(
  db_url="postgresql://localhost/synapse",
  chunk_size=500,
  default_levels="100,50,25",
  number_of_chunks=100
)
```

To see any output from the compressor, logging must first be setup from Python.

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


## Using as Python library

The compressor can also be built into a python library as it uses PyO3. It can be
built and installed into the current virtual environment by running `maturin develop`:

1. Create a virtual environment in the place you want to use the compressor from
(if it doesn't already exist).  
`$ virtualenv -p python3 venv`

2. Activate the virtual environment  
`$ source venv/bin/activate`

3. Build and install the library  
`$ cd /home/synapse/rust-synapse-compress-state`  
`$ pip install maturin`  
`$ maturin develop`


All the same running options are available, see the `Config` struct in `lib.rs`
for the names of each argument. All arguments other than `db_url` and `room_id`
are optional.

The following code does exactly the same as the command-line example from above:

```python
import synapse_compress_state as comp

comp.run_compression(
  db_url="postgresql://localhost/synapse",
  room_id="!some_room:example.com",
  output_file="out.sql",
  transactions=True
)
```

To see any output from the compressor, logging must first be setup from Python.

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
remote connections.

To `/etc/postgresql/12/main/pg_hba.conf` add the following:

```
#   TYPE    DATABASE  USER            ADDRESS    METHOD
    host    synapse   synapse_user    ADDR        md5
```
Substitute `ADDR` with the IP address of the machine you wish to connect from followed by `/32` (if it is
an ipv4 address) or `/128` if it's an ipv6 address (e.g. `234.123.42.555/32` or `3a:23:5d::23:37/128`).

If you want to be able to connect from any address (**for testing ONLY**) then you can use
`0.0.0.0/0` or `::/0`

To `/etc/postgresql/12/main/postgresql.conf` add the following:

```
listen_addresses = 'localhost, IP_ADDR'
```
Substitute `IP_ADDR` with your ip address (WITHOUT the `/32` or `/128` that was used in `pg_hba.conf`).

If you want to allow connections from any address (**for testing ONLY**) then substitute `IP_ADDR` with `*`

### Non default port

By default, it tries to connect to a Postgres server running on port 5432. If you have configured your
database to use a different port then the URL will take the following form:

`postgresql://synapse_user:synapse_password@mydomain:PORT/synapse`

See [the postgres crate documentation](https://docs.rs/tokio-postgres/0.7.2/tokio_postgres/config/struct.Config.html)
for the full list of options.

## Printing debugging logs

The amount of output the tools produce can be altered by setting the COMPRESSOR_LOG_LEVEL 
environment variable to something. 

To get more logs when running the auto_compressor tool try the following:

```
$ COMPRESSOR_LOG_LEVEL=debug auto_compressor -p postgresql://user:pass@localhost/synapse -c 5 -l '100,50,25' -n 5000
```

If you want to suppress all the debugging info you are getting from the 
Postgres client then try:

```
COMPRESSOR_LOG_LEVEL=auto_compressor=debug,synapse_compress_state=debug auto_compressor [etc.]
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
