-- Create the abis table to store ABI definitions
CREATE TABLE IF NOT EXISTS abis (
    name TEXT PRIMARY KEY NOT NULL,
    abi_content TEXT NOT NULL,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- Create an index for efficient queries
CREATE INDEX IF NOT EXISTS idx_abis_name ON abis(name);

-- Migrate existing ABI data from monitors table to abis table
-- This creates entries in the abis table for any unique ABI found in monitors
INSERT OR IGNORE INTO abis (name, abi_content)
SELECT DISTINCT 
    abi as name,
    abi as abi_content
FROM monitors 
WHERE abi IS NOT NULL AND abi != '';

-- Note: The monitors.abi column currently stores the full ABI JSON.
-- After this migration, we'll update the application logic to store only the ABI name
-- and reference the abis table via foreign key semantics (enforced at application level).
