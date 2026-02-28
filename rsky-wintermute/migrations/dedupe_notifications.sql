-- Deduplicate notifications and add unique constraint
-- WARNING: This migration operates on 1+ billion rows and will take many hours

-- Step 1: Create a temporary table with distinct notifications
-- Using ROW_NUMBER() to keep only the first occurrence (lowest id) of each duplicate
CREATE TABLE bsky.notification_deduped AS
SELECT id, did, "recordUri", "recordCid", author, reason, "reasonSubject", "sortAt"
FROM (
    SELECT *,
           ROW_NUMBER() OVER (PARTITION BY did, "recordUri", reason ORDER BY id) as rn
    FROM bsky.notification
) sub
WHERE rn = 1;

-- Step 2: Create indexes on the new table (before swap to minimize downtime)
CREATE INDEX notification_deduped_did_sortat_idx ON bsky.notification_deduped (did, "sortAt");
ALTER TABLE bsky.notification_deduped ADD PRIMARY KEY (id);

-- Step 3: Add the unique constraint
CREATE UNIQUE INDEX notification_deduped_unique_idx
ON bsky.notification_deduped (did, "recordUri", reason);

-- Step 4: Swap the tables
-- IMPORTANT: Do this during a maintenance window
BEGIN;
ALTER TABLE bsky.notification RENAME TO notification_old;
ALTER TABLE bsky.notification_deduped RENAME TO notification;
-- Update the sequence to continue from the max id
SELECT setval('bsky.notification_id_seq', (SELECT MAX(id) FROM bsky.notification));
COMMIT;

-- Step 5: Drop the old table (after verifying everything works)
-- DROP TABLE bsky.notification_old;

-- Verification queries:
-- SELECT COUNT(*) FROM bsky.notification;
-- SELECT COUNT(*) FROM bsky.notification_old;
-- SELECT (SELECT COUNT(*) FROM bsky.notification_old) - (SELECT COUNT(*) FROM bsky.notification) as duplicates_removed;
