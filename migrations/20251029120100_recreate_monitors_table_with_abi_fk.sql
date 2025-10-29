-- Drop the existing monitors table to redefine it
DROP TABLE IF EXISTS monitors;

-- Recreate the monitors table with the new schema, including a foreign key to the abis table
CREATE TABLE monitors (
    monitor_id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    network TEXT NOT NULL,
    address TEXT, -- Nullable to support transaction-level monitors
    filter_script TEXT NOT NULL,
    actions TEXT NOT NULL DEFAULT '[]',
    abi_name TEXT,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY(abi_name) REFERENCES abis(name)
);

-- Create indexes for efficient queries
CREATE INDEX IF NOT EXISTS idx_monitors_network ON monitors(network);
CREATE INDEX IF NOT EXISTS idx_monitors_address ON monitors(address);
CREATE INDEX IF NOT EXISTS idx_monitors_network_address ON monitors(network, address);
CREATE INDEX IF NOT EXISTS idx_monitors_abi_name ON monitors(abi_name);
