-- Add migration script here
CREATE TABLE IF NOT EXISTS monitors (
    monitor_id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    network TEXT NOT NULL,
    address TEXT NOT NULL,
    filter_script TEXT NOT NULL,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- Create indexes for efficient queries
CREATE INDEX IF NOT EXISTS idx_monitors_network ON monitors(network);
CREATE INDEX IF NOT EXISTS idx_monitors_address ON monitors(address);
CREATE INDEX IF NOT EXISTS idx_monitors_network_address ON monitors(network, address);
