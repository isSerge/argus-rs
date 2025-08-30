-- A migration to create the 'application_state' table for generic key-value storage.

CREATE TABLE IF NOT EXISTS application_state (
    key TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL, -- JSON representation of the state
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);
