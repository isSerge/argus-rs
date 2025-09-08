-- Add decode_calldata to monitors table
ALTER TABLE monitors
ADD COLUMN decode_calldata BOOLEAN NOT NULL DEFAULT FALSE;
