-- This file should undo anything in `up.sql`
ALTER TABLE sub_state 
ALTER COLUMN cursor TYPE INTEGER;