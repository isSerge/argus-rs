-- Add on_match column to monitors table
ALTER TABLE monitors ADD COLUMN on_match TEXT NOT NULL DEFAULT '[]';
