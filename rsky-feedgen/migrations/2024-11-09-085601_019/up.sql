-- Step 1: Add the new column with the same data type
ALTER TABLE post
ADD COLUMN "createdAt" character varying DEFAULT to_char(now(), 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"');

-- Step 2: Update the new column to copy values from the existing column
UPDATE post
SET "createdAt" = "indexedAt";

-- Step 3: Alter the column to set it as NOT NULL
ALTER TABLE post
ALTER COLUMN "createdAt" SET NOT NULL;