-- This file should undo anything in `up.sql`
ALTER TABLE post
DROP COLUMN IF EXISTS createdAt;