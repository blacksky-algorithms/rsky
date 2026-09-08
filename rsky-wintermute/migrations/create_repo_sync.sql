-- Last observed commit per repo on the live firehose, for sync 1.1 gap
-- detection (see rsky-wintermute/src/ingester/sync11.rs).
-- wintermute-only: the appview/dataplane never reads this table. Nothing else creates it.
--
--   did        the repo
--   rev        rev of the last commit applied from the firehose
--   data_cid   that commit's MST root; the next #commit's prevData must equal it
--   host       relay the commit arrived from
CREATE TABLE IF NOT EXISTS bsky.repo_sync (
    did        varchar PRIMARY KEY,
    rev        varchar NOT NULL,
    data_cid   varchar NOT NULL,
    host       varchar,
    updated_at timestamptz NOT NULL DEFAULT now()
);
