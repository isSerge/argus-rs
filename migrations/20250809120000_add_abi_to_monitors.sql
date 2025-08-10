-- Add the 'abi' column to the 'monitors' table to store the full JSON ABI.
ALTER TABLE monitors
ADD COLUMN abi TEXT;
