-- Add migration script here
CREATE TABLE IF NOT EXISTS abis (
    name TEXT PRIMARY KEY NOT NULL,
    abi TEXT NOT NULL,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_abis_name ON abis(name);
