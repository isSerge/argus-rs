-- Add unique constraint on (network, name) to ensure monitor names are unique per network
-- This enables reliable identification and updates by name
CREATE UNIQUE INDEX IF NOT EXISTS idx_monitors_network_name_unique ON monitors(network, name);

-- Add status column to support pausing/resuming monitors without deletion
-- Default to 'active' for all existing monitors
ALTER TABLE monitors ADD COLUMN status TEXT NOT NULL DEFAULT 'active' CHECK(status IN ('active', 'paused'));

-- Create index on status for efficient filtering
CREATE INDEX IF NOT EXISTS idx_monitors_status ON monitors(status);
