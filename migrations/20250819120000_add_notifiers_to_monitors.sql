-- Add notifiers column to monitors table
ALTER TABLE monitors ADD COLUMN notifiers TEXT NOT NULL DEFAULT '[]';
