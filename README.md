# Compress Synapse State Tables

An experimental tool that reads in the rows from `state_groups_state` and
`state_group_edges` tables for a particular room and calculates the changes that
could be made that (hopefully) will significantly reduce the number of rows.

This tool currently *does not* write to the database in any way, so should be
safe to run. If the `-o` option is specified then SQL will be written to the
given file that would change the tables to match the calculated state. (Note
that if `-t` is given then each change to a particular state group is wrapped
in a transaction).

The tool will also ensure that the generated state deltas do give the same state
as the existing state deltas.

## Algorithm

The algorithm works by attempting to create a tree of deltas, produced by
appending state groups to different "levels". Each level has a maximum size, where
each state group is appended to the lowest level that is not full.

This produces a graph that looks approximately like the following, in the case
of having two levels with the bottom level (L1) having a maximum size of 3:

```
L2 <-------------------- L2 <---------- ...
^--- L1 <--- L1 <--- L1  ^--- L1 <--- L1 <--- L1
```

The sizes and number of levels used can be controlled via `-l`.

**Note**: Increasing the sum of the sizes of levels will increase the time it
takes for to query the full state of a given state group. By default Synapse
attempts to keep this below 100.


## Example usage

```
$ synapse-compress-state -p "postgresql://localhost/synapse" -r '!some_room:example.com' -o out.sql -t
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
