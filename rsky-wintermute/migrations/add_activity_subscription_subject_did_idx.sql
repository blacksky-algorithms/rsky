-- subscribed-post fan-out looks up subscribers by author on every indexed post;
-- the existing keys on this table all lead with creator.
-- Run outside a transaction (CONCURRENTLY). Safe before or after the binary deploy.

CREATE INDEX CONCURRENTLY IF NOT EXISTS activity_subscription_subject_did_idx
ON bsky.activity_subscription ("subjectDid");
