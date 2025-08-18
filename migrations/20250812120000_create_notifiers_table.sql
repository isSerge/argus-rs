-- A migration to create the 'notifiers' table.

CREATE TABLE IF NOT EXISTS notifiers (
    notifier_id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    network_id TEXT NOT NULL,
    config TEXT NOT NULL, -- JSON representation of the NotifierConfig
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(name, network_id)
);
