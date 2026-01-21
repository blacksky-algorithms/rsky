-- Add unique constraint to notification table to prevent duplicates
-- This migration should be run AFTER deduplication (see dedupe_notifications.sql)
-- Or on a database with few/no duplicates

-- Step 1: Delete duplicates, keeping the row with the lowest id
-- This uses a CTE with ROW_NUMBER to identify duplicates
DELETE FROM bsky.notification
WHERE id IN (
    SELECT id FROM (
        SELECT id,
               ROW_NUMBER() OVER (PARTITION BY did, "recordUri", reason ORDER BY id) as rn
        FROM bsky.notification
    ) sub
    WHERE rn > 1
);

-- Step 2: Add the unique constraint
-- Using CONCURRENTLY to avoid blocking other operations
CREATE UNIQUE INDEX CONCURRENTLY IF NOT EXISTS notification_did_recorduri_reason_unique_idx
ON bsky.notification (did, "recordUri", reason);

-- Verify the constraint was created
SELECT indexname, indexdef
FROM pg_indexes
WHERE schemaname = 'bsky' AND tablename = 'notification';
