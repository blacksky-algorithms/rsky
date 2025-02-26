-- Revert the post table
ALTER TABLE post
  ALTER COLUMN "createdAt" DROP DEFAULT;

-- Convert back to character varying using a text cast
ALTER TABLE post
  ALTER COLUMN "createdAt" TYPE character varying USING "createdAt"::text,
  ALTER COLUMN "indexedAt" TYPE character varying USING "indexedAt"::text;

-- Reapply the original default for "createdAt"
ALTER TABLE post
  ALTER COLUMN "createdAt" SET DEFAULT to_char(now(), 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"');

UPDATE post
SET "createdAt" = "indexedAt";

-- Set "createdAt" as NOT NULL
ALTER TABLE post
  ALTER COLUMN "createdAt" SET NOT NULL;

-- Revert the like table (note the quotes because "like" is a reserved word)
ALTER TABLE "like"
  ALTER COLUMN "createdAt" TYPE character varying
  USING "createdAt"::text;
ALTER TABLE "like"
  ALTER COLUMN "indexedAt" TYPE character varying
  USING "indexedAt"::text;

-- Revert the video table
ALTER TABLE video
  ALTER COLUMN "createdAt" TYPE character varying
  USING "createdAt"::text;
ALTER TABLE video
  ALTER COLUMN "indexedAt" TYPE character varying
  USING "indexedAt"::text;

-- Revert the image table
ALTER TABLE image
  ALTER COLUMN "createdAt" TYPE character varying
  USING "createdAt"::text;
ALTER TABLE image
  ALTER COLUMN "indexedAt" TYPE character varying
  USING "indexedAt"::text;