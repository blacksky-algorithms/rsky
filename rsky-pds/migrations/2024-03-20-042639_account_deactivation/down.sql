-- This file should undo anything in `up.sql`
ALTER TABLE pds.actor
    DROP COLUMN IF EXISTS deactivatedAt,
    DROP COLUMN IF EXISTS deleteAfter;