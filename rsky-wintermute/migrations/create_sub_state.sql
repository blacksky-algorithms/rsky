-- Per-stream cursor/sequence state for the firehose + label subscriptions.
-- wintermute-only: the appview/dataplane never reads this table. Nothing else creates it.
CREATE TABLE IF NOT EXISTS bsky.sub_state (
    service text PRIMARY KEY,
    cursor bigint NOT NULL
);
