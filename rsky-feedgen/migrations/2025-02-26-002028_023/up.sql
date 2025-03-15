-- Drop defaults on post table so they don’t interfere
ALTER TABLE post ALTER COLUMN "createdAt" DROP DEFAULT;

-- Update the post table
ALTER TABLE post
  ALTER COLUMN "createdAt" TYPE timestamptz
  USING "createdAt"::timestamptz;
ALTER TABLE post
  ALTER COLUMN "indexedAt" TYPE timestamptz
  USING "indexedAt"::timestamptz;
ALTER TABLE post ALTER COLUMN "createdAt" SET DEFAULT now();

-- Update the like table (note the quotes because like is reserved)
ALTER TABLE "like"
  ALTER COLUMN "createdAt" TYPE timestamptz
  USING "createdAt"::timestamptz;
ALTER TABLE "like"
  ALTER COLUMN "indexedAt" TYPE timestamptz
  USING "indexedAt"::timestamptz;

-- Update the video table
ALTER TABLE video
  ALTER COLUMN "createdAt" TYPE timestamptz
  USING "createdAt"::timestamptz;
ALTER TABLE video
  ALTER COLUMN "indexedAt" TYPE timestamptz
  USING "indexedAt"::timestamptz;

-- Update the image table
ALTER TABLE image
  ALTER COLUMN "createdAt" TYPE timestamptz
  USING "createdAt"::timestamptz;
ALTER TABLE image
  ALTER COLUMN "indexedAt" TYPE timestamptz
  USING "indexedAt"::timestamptz;