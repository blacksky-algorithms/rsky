-- Batched backfill for large tables (prevents long locks)
-- Run each batch separately, adjusting LIMIT/OFFSET as needed

-- Check total counts first
SELECT
    'images' as type,
    COUNT(*) as total
FROM record
WHERE uri LIKE 'at://%/app.bsky.feed.post/%'
  AND (
    (json::jsonb)->'embed'->>'$type' = 'app.bsky.embed.images'
    OR (
      (json::jsonb)->'embed'->>'$type' = 'app.bsky.embed.recordWithMedia'
      AND (json::jsonb)->'embed'->'media'->>'$type' = 'app.bsky.embed.images'
    )
  )
UNION ALL
SELECT
    'videos' as type,
    COUNT(*) as total
FROM record
WHERE uri LIKE 'at://%/app.bsky.feed.post/%'
  AND (
    (json::jsonb)->'embed'->>'$type' = 'app.bsky.embed.video'
    OR (
      (json::jsonb)->'embed'->>'$type' = 'app.bsky.embed.recordWithMedia'
      AND (json::jsonb)->'embed'->'media'->>'$type' = 'app.bsky.embed.video'
    )
  );

-- Batched image backfill (adjust LIMIT and run multiple times)
-- Batch 1: Direct image embeds
WITH batch AS (
    SELECT r.uri, r.json
    FROM record r
    LEFT JOIN post_embed_image pei ON pei."postUri" = r.uri
    WHERE r.uri LIKE 'at://%/app.bsky.feed.post/%'
      AND (r.json::jsonb)->'embed'->>'$type' = 'app.bsky.embed.images'
      AND pei."postUri" IS NULL
    LIMIT 10000
)
INSERT INTO post_embed_image ("postUri", position, "imageCid", alt)
SELECT
    b.uri as "postUri",
    (img_idx.idx - 1)::text as position,
    img.value->'image'->'ref'->>'$link' as "imageCid",
    COALESCE(img.value->>'alt', '') as alt
FROM batch b,
    jsonb_array_elements((b.json::jsonb)->'embed'->'images') WITH ORDINALITY AS img(value, idx),
    LATERAL (SELECT img.idx as idx) img_idx
WHERE img.value->'image'->'ref'->>'$link' IS NOT NULL
ON CONFLICT DO NOTHING;

-- Batch 2: Images in recordWithMedia
WITH batch AS (
    SELECT r.uri, r.json
    FROM record r
    LEFT JOIN post_embed_image pei ON pei."postUri" = r.uri
    WHERE r.uri LIKE 'at://%/app.bsky.feed.post/%'
      AND (r.json::jsonb)->'embed'->>'$type' = 'app.bsky.embed.recordWithMedia'
      AND (r.json::jsonb)->'embed'->'media'->>'$type' = 'app.bsky.embed.images'
      AND pei."postUri" IS NULL
    LIMIT 10000
)
INSERT INTO post_embed_image ("postUri", position, "imageCid", alt)
SELECT
    b.uri as "postUri",
    (img_idx.idx - 1)::text as position,
    img.value->'image'->'ref'->>'$link' as "imageCid",
    COALESCE(img.value->>'alt', '') as alt
FROM batch b,
    jsonb_array_elements((b.json::jsonb)->'embed'->'media'->'images') WITH ORDINALITY AS img(value, idx),
    LATERAL (SELECT img.idx as idx) img_idx
WHERE img.value->'image'->'ref'->>'$link' IS NOT NULL
ON CONFLICT DO NOTHING;

-- Batched video backfill
-- Batch 1: Direct video embeds
WITH batch AS (
    SELECT r.uri, r.json
    FROM record r
    LEFT JOIN post_embed_video pev ON pev."postUri" = r.uri
    WHERE r.uri LIKE 'at://%/app.bsky.feed.post/%'
      AND (r.json::jsonb)->'embed'->>'$type' = 'app.bsky.embed.video'
      AND pev."postUri" IS NULL
    LIMIT 10000
)
INSERT INTO post_embed_video ("postUri", "videoCid", alt)
SELECT
    b.uri as "postUri",
    (b.json::jsonb)->'embed'->'video'->'ref'->>'$link' as "videoCid",
    (b.json::jsonb)->'embed'->>'alt' as alt
FROM batch b
WHERE (b.json::jsonb)->'embed'->'video'->'ref'->>'$link' IS NOT NULL
ON CONFLICT DO NOTHING;

-- Batch 2: Videos in recordWithMedia
WITH batch AS (
    SELECT r.uri, r.json
    FROM record r
    LEFT JOIN post_embed_video pev ON pev."postUri" = r.uri
    WHERE r.uri LIKE 'at://%/app.bsky.feed.post/%'
      AND (r.json::jsonb)->'embed'->>'$type' = 'app.bsky.embed.recordWithMedia'
      AND (r.json::jsonb)->'embed'->'media'->>'$type' = 'app.bsky.embed.video'
      AND pev."postUri" IS NULL
    LIMIT 10000
)
INSERT INTO post_embed_video ("postUri", "videoCid", alt)
SELECT
    b.uri as "postUri",
    (b.json::jsonb)->'embed'->'media'->'video'->'ref'->>'$link' as "videoCid",
    (b.json::jsonb)->'embed'->'media'->>'alt' as alt
FROM batch b
WHERE (b.json::jsonb)->'embed'->'media'->'video'->'ref'->>'$link' IS NOT NULL
ON CONFLICT DO NOTHING;

-- Check remaining after each batch
SELECT
    'remaining_images' as metric,
    COUNT(*) as count
FROM record r
LEFT JOIN post_embed_image pei ON pei."postUri" = r.uri
WHERE r.uri LIKE 'at://%/app.bsky.feed.post/%'
  AND (
    (r.json::jsonb)->'embed'->>'$type' = 'app.bsky.embed.images'
    OR (
      (r.json::jsonb)->'embed'->>'$type' = 'app.bsky.embed.recordWithMedia'
      AND (r.json::jsonb)->'embed'->'media'->>'$type' = 'app.bsky.embed.images'
    )
  )
  AND pei."postUri" IS NULL;
