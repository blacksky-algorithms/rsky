-- Backfill post_embed_image and post_embed_video tables from existing record JSON
-- Run this migration after deploying the indexer fix

-- Step 1: Backfill post_embed_image from app.bsky.embed.images
INSERT INTO post_embed_image ("postUri", position, "imageCid", alt)
SELECT
    r.uri as "postUri",
    (img_idx.idx - 1)::text as position,
    img.value->'image'->'ref'->>'$link' as "imageCid",
    COALESCE(img.value->>'alt', '') as alt
FROM record r,
    jsonb_array_elements((r.json::jsonb)->'embed'->'images') WITH ORDINALITY AS img(value, idx),
    LATERAL (SELECT img.idx as idx) img_idx
WHERE r.uri LIKE 'at://%/app.bsky.feed.post/%'
  AND (r.json::jsonb)->'embed'->>'$type' = 'app.bsky.embed.images'
  AND img.value->'image'->'ref'->>'$link' IS NOT NULL
ON CONFLICT DO NOTHING;

-- Step 2: Backfill post_embed_image from app.bsky.embed.recordWithMedia (images in media)
INSERT INTO post_embed_image ("postUri", position, "imageCid", alt)
SELECT
    r.uri as "postUri",
    (img_idx.idx - 1)::text as position,
    img.value->'image'->'ref'->>'$link' as "imageCid",
    COALESCE(img.value->>'alt', '') as alt
FROM record r,
    jsonb_array_elements((r.json::jsonb)->'embed'->'media'->'images') WITH ORDINALITY AS img(value, idx),
    LATERAL (SELECT img.idx as idx) img_idx
WHERE r.uri LIKE 'at://%/app.bsky.feed.post/%'
  AND (r.json::jsonb)->'embed'->>'$type' = 'app.bsky.embed.recordWithMedia'
  AND (r.json::jsonb)->'embed'->'media'->>'$type' = 'app.bsky.embed.images'
  AND img.value->'image'->'ref'->>'$link' IS NOT NULL
ON CONFLICT DO NOTHING;

-- Step 3: Backfill post_embed_video from app.bsky.embed.video
INSERT INTO post_embed_video ("postUri", "videoCid", alt)
SELECT
    r.uri as "postUri",
    (r.json::jsonb)->'embed'->'video'->'ref'->>'$link' as "videoCid",
    (r.json::jsonb)->'embed'->>'alt' as alt
FROM record r
WHERE r.uri LIKE 'at://%/app.bsky.feed.post/%'
  AND (r.json::jsonb)->'embed'->>'$type' = 'app.bsky.embed.video'
  AND (r.json::jsonb)->'embed'->'video'->'ref'->>'$link' IS NOT NULL
ON CONFLICT DO NOTHING;

-- Step 4: Backfill post_embed_video from app.bsky.embed.recordWithMedia (video in media)
INSERT INTO post_embed_video ("postUri", "videoCid", alt)
SELECT
    r.uri as "postUri",
    (r.json::jsonb)->'embed'->'media'->'video'->'ref'->>'$link' as "videoCid",
    (r.json::jsonb)->'embed'->'media'->>'alt' as alt
FROM record r
WHERE r.uri LIKE 'at://%/app.bsky.feed.post/%'
  AND (r.json::jsonb)->'embed'->>'$type' = 'app.bsky.embed.recordWithMedia'
  AND (r.json::jsonb)->'embed'->'media'->>'$type' = 'app.bsky.embed.video'
  AND (r.json::jsonb)->'embed'->'media'->'video'->'ref'->>'$link' IS NOT NULL
ON CONFLICT DO NOTHING;

-- Verification queries (run these to check progress)
-- SELECT COUNT(*) FROM post_embed_image;
-- SELECT COUNT(*) FROM post_embed_video;
-- SELECT COUNT(*) FROM record WHERE uri LIKE 'at://%/app.bsky.feed.post/%' AND (json::jsonb)->'embed'->>'$type' = 'app.bsky.embed.images';
