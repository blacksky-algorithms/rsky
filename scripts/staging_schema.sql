-- Staging schema for offline build + sorted merge backfill.
-- UNLOGGED tables, NO indexes, NO constraints, NO generated columns.
-- Pure sequential append for maximum COPY throughput.

CREATE SCHEMA IF NOT EXISTS bsky;
SET search_path TO bsky;

CREATE UNLOGGED TABLE staging_actor (
    did text NOT NULL
);

CREATE UNLOGGED TABLE staging_record (
    uri text NOT NULL,
    cid text NOT NULL,
    did text NOT NULL,
    json text,
    rev text,
    indexed_at text NOT NULL
);

CREATE UNLOGGED TABLE staging_post (
    uri text NOT NULL,
    cid text NOT NULL,
    creator text NOT NULL,
    text text,
    created_at text NOT NULL,
    indexed_at text NOT NULL
);

CREATE UNLOGGED TABLE staging_like (
    uri text NOT NULL,
    cid text NOT NULL,
    creator text NOT NULL,
    subject text NOT NULL,
    subject_cid text NOT NULL,
    created_at text NOT NULL,
    indexed_at text NOT NULL
);

CREATE UNLOGGED TABLE staging_follow (
    uri text NOT NULL,
    cid text NOT NULL,
    creator text NOT NULL,
    subject_did text NOT NULL,
    created_at text NOT NULL,
    indexed_at text NOT NULL
);

CREATE UNLOGGED TABLE staging_repost (
    uri text NOT NULL,
    cid text NOT NULL,
    creator text NOT NULL,
    subject text NOT NULL,
    subject_cid text NOT NULL,
    created_at text NOT NULL,
    indexed_at text NOT NULL
);

CREATE UNLOGGED TABLE staging_feed_item (
    type text NOT NULL,
    uri text NOT NULL,
    cid text NOT NULL,
    post_uri text NOT NULL,
    originator_did text NOT NULL,
    sort_at text NOT NULL
);

CREATE UNLOGGED TABLE staging_block (
    uri text NOT NULL,
    cid text NOT NULL,
    creator text NOT NULL,
    subject text NOT NULL,
    created_at text NOT NULL,
    indexed_at text NOT NULL
);

CREATE UNLOGGED TABLE staging_profile (
    uri text NOT NULL,
    cid text NOT NULL,
    creator text NOT NULL,
    display_name text,
    description text,
    avatar_cid text,
    banner_cid text,
    indexed_at text NOT NULL
);

CREATE UNLOGGED TABLE staging_post_embed_image (
    post_uri text NOT NULL,
    position text NOT NULL,
    image_cid text NOT NULL,
    alt text NOT NULL
);

CREATE UNLOGGED TABLE staging_post_embed_video (
    post_uri text NOT NULL,
    video_cid text NOT NULL,
    alt text
);
