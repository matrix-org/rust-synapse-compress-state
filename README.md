# Compress Synapse State Tables

An experimental tool that reads in the rows from `state_groups_state` and
`state_group_edges` tables for a particular room and calculates the changes that
could be made that (hopefully) will signifcantly reduce the number of rows.

This tool currently *does not* write to the database in any way, so should be
safe to run.


## Example

```
$ cargo run --release -- -p "postgresql://localhost/synapse" -r '!some_room:example.com'
   Compiling rust-synapse-compress-state v0.1.0 (file:///home/erikj/git/rust-synapse-compress-state)
    Finished release [optimized] target(s) in 2.39s
     Running `target/release/rust-synapse-compress-state -p 'postgresql://localhost/synapse' -r '!some_room:example.com'`
Missing 11 state groups
Number of entries: 25694
Number of rows: 356650
Number of rows compressed: 41068
New state map matches old one
```
