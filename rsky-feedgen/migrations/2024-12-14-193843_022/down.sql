-- This file should undo anything in `up.sql`
ALTER TABLE post
DROP CONSTRAINT IF EXISTS no_nulls_in_labels,
DROP COLUMN IF EXISTS labels;