-- Add a new `action_id` column to the `actions` table and set it as the primary key.
-- This is done by creating a new table with the desired schema, copying the data,
-- and then replacing the old table with the new one.

-- Step 1: Create a new table with the `action_id` as a primary key.
CREATE TABLE IF NOT EXISTS actions_new (
    action_id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    network_id TEXT NOT NULL,
    config TEXT NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Step 2: Copy the data from the old table to the new table.
INSERT INTO actions_new (name, network_id, config, created_at, updated_at)
SELECT name, network_id, config, created_at, updated_at FROM actions;

-- Step 3: Drop the old table.
DROP TABLE actions;

-- Step 4: Rename the new table to the original name.
ALTER TABLE actions_new RENAME TO actions;

-- Step 5: Create a unique index on name and network_id to maintain the original constraint.
CREATE UNIQUE INDEX IF NOT EXISTS idx_actions_name_network_id ON actions (name, network_id);