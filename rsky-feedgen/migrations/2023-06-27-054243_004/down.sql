-- This file should undo anything in `up.sql`
ALTER TABLE post 
ALTER COLUMN sequence TYPE NUMERIC;