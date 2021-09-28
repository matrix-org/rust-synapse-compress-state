# Compression algorithm

## What is state?
State is things like who is in a room, what the room topic/name is, who has
what privilege levels etc. Synapse keeps track of it for various reasons such as
spotting invalid events (e.g. ones sent by banned users) and providing room membership
information to clients.

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