#!/bin/sh

#N.B. the database setup comes from:
#https://github.com/matrix-org/synapse/blob/develop/synapse/storage/schema/state/full_schemas/54/full.sql

# Setup the required tables for testing
psql --username "$POSTGRES_USER" --dbname "$POSTGRES_DB" <<SQLCODE

CREATE TABLE state_groups (
    id BIGINT PRIMARY KEY,
    room_id TEXT NOT NULL,
    event_id TEXT NOT NULL
);

CREATE TABLE state_groups_state (
    state_group BIGINT NOT NULL,
    room_id TEXT NOT NULL,
    type TEXT NOT NULL,
    state_key TEXT NOT NULL,
    event_id TEXT NOT NULL
);

CREATE TABLE state_group_edges (
    state_group BIGINT NOT NULL,
    prev_state_group BIGINT NOT NULL
);

SQLCODE
