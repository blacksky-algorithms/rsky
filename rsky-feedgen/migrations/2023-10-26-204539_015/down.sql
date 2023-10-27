-- This file should undo anything in `up.sql`
ALTER TABLE post 
DROP COLUMN IF EXISTS author,
DROP COLUMN IF EXISTS externalUri,
DROP COLUMN IF EXISTS externalTitle,
DROP COLUMN IF EXISTS externalDescription,
DROP COLUMN IF EXISTS externalThumb,
DROP COLUMN IF EXISTS quoteCid,
DROP COLUMN IF EXISTS quoteUri;