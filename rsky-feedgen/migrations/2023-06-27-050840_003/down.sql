-- This file should undo anything in `up.sql`
ALTER TABLE post 
DROP CONSTRAINT IF EXISTS unique_sequence;

ALTER TABLE post 
DROP COLUMN IF EXISTS prev,
DROP COLUMN IF EXISTS sequence;