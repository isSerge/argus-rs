-- Add actions column to monitors table
ALTER TABLE monitors ADD COLUMN actions TEXT NOT NULL DEFAULT '[]';
