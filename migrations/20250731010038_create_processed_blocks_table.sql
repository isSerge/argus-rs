-- Add migration script here
CREATE TABLE IF NOT EXISTS processed_blocks (
    network_id TEXT PRIMARY KEY NOT NULL,
    block_number BIGINT NOT NULL
);