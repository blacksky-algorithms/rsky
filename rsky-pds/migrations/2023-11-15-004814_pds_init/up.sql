-- Create PDS schema
CREATE SCHEMA IF NOT EXISTS pds;

/* Based heavily on account-manager, did-cache, sequencer, and actor-store migrations
    from the canonical TS implementation. */

-- account-manager implementation
-- Create App Password Table
CREATE TABLE IF NOT EXISTS pds.app_password (
    did character varying NOT NULL,
    name character varying NOT NULL,
    "password" character varying NOT NULL,
    "createdAt" character varying NOT NULL
);

ALTER TABLE ONLY pds.app_password
    DROP CONSTRAINT IF EXISTS app_password_pkey;
ALTER TABLE ONLY pds.app_password
    ADD CONSTRAINT app_password_pkey PRIMARY KEY (did, name);

-- Create Invite Code Table
CREATE TABLE IF NOT EXISTS pds.invite_code (
    code character varying PRIMARY KEY,
    "availableUses" integer NOT NULL,
    disabled smallint NOT NULL DEFAULT 0,
    "forAccount" character varying NOT NULL,
    "createdBy" character varying NOT NULL,
    "createdAt" character varying NOT NULL
);
CREATE INDEX invite_code_for_user_idx
    ON pds.invite_code("forAccount");

-- Create Invite Code Use Table
CREATE TABLE IF NOT EXISTS pds.invite_code_use (
    code character varying NOT NULL,
    "usedBy" character varying NOT NULL,
    "usedAt" character varying NOT NULL
);

ALTER TABLE ONLY pds.invite_code_use
    DROP CONSTRAINT IF EXISTS invite_code_use_pkey;
ALTER TABLE ONLY pds.invite_code_use
    ADD CONSTRAINT invite_code_use_pkey PRIMARY KEY (code, "usedBy");

-- Create Refresh Token Table
CREATE TABLE IF NOT EXISTS pds.refresh_token (
    id character varying PRIMARY KEY,
    did character varying NOT NULL,
    "expiresAt" character varying NOT NULL,
    "nextId" character varying,
    "appPasswordName" character varying
);
CREATE INDEX refresh_token_did_idx -- Aids in refresh token cleanup
    ON pds.refresh_token(did);

-- Create Actor Table
CREATE TABLE IF NOT EXISTS pds.actor (
    did character varying PRIMARY KEY,
    handle character varying,
    "createdAt" character varying NOT NULL,
    "takedownRef" character varying
);
CREATE UNIQUE INDEX actor_handle_lower_idx
    ON pds.actor (LOWER(handle));
CREATE INDEX actor_cursor_idx
    ON pds.actor("createdAt", did);

-- Create Account Table
CREATE TABLE IF NOT EXISTS pds.account (
    did character varying PRIMARY KEY,
    email character varying NOT NULL,
    "recoveryKey" character varying, -- For storing Bring Your Own Key
    "password" character varying NOT NULL,
    "createdAt" character varying NOT NULL,
	"invitesDisabled" smallint NOT NULL DEFAULT 0,
	"emailConfirmedAt" character varying
);
CREATE UNIQUE INDEX account_email_lower_idx
	ON pds.account (LOWER(email));
CREATE INDEX account_cursor_idx
	ON pds.account("createdAt", did);

-- Create Email Token Table
CREATE TABLE IF NOT EXISTS pds.email_token (
    purpose character varying NOT NULL,
    did character varying NOT NULL,
    token character varying NOT NULL,
    "requestedAt" character varying NOT NULL
);
ALTER TABLE ONLY pds.email_token
    DROP CONSTRAINT IF EXISTS email_token_pkey;
ALTER TABLE ONLY pds.email_token
    ADD CONSTRAINT email_token_pkey PRIMARY KEY (purpose, did);
CREATE UNIQUE INDEX email_token_purpose_token_unique
	ON pds.email_token (purpose, token);


-- actor-store implementation
-- Create Repo Root Table
CREATE TABLE IF NOT EXISTS pds.repo_root (
    did character varying PRIMARY KEY,
    cid character varying NOT NULL,
    rev character varying NOT NULL,
    "indexedAt" character varying NOT NULL
);

-- Create Repo Block Table
CREATE TABLE IF NOT EXISTS pds.repo_block (
    cid character varying NOT NULL,
    did character varying NOT NULL,
    "repoRev" character varying NOT NULL,
    size integer NOT NULL,
    content bytea NOT NULL
);
ALTER TABLE ONLY pds.repo_block
    ADD CONSTRAINT repo_block_pkey PRIMARY KEY (cid, did);
CREATE INDEX repo_block_repo_rev_idx
	ON pds.repo_block("repoRev", cid);

-- Create Record Table
CREATE TABLE IF NOT EXISTS pds.record (
    uri character varying PRIMARY KEY,
    cid character varying NOT NULL,
    did character varying NOT NULL,
    collection character varying NOT NULL,
    "rkey" character varying NOT NULL,
    "repoRev" character varying,
    "indexedAt" character varying NOT NULL,
    "takedownRef" character varying
);
CREATE INDEX record_did_cid_idx
	ON pds.record(cid);
CREATE INDEX record_did_collection_idx
	ON pds.record(collection);
CREATE INDEX record_repo_rev_idx
	ON pds.record("repoRev");

-- Create Blob Table
CREATE TABLE IF NOT EXISTS pds.blob (
    cid character varying NOT NULL,
    did character varying NOT NULL,
    "mimeType" character varying NOT NULL,
    size integer NOT NULL,
    "tempKey" character varying,
    width integer,
    height integer,
    "createdAt" character varying NOT NULL,
    "takedownRef" character varying
);
ALTER TABLE ONLY pds.blob
    ADD CONSTRAINT blob_pkey PRIMARY KEY (cid, did);
CREATE INDEX blob_tempkey_idx
	ON pds.blob("tempKey");

-- Create Record Blob Table
CREATE TABLE IF NOT EXISTS pds.record_blob (
    "blobCid" character varying NOT NULL,
    "recordUri" character varying NOT NULL,
    did character varying NOT NULL
);
ALTER TABLE ONLY pds.record_blob
    DROP CONSTRAINT IF EXISTS record_blob_pkey;
ALTER TABLE ONLY pds.record_blob
    ADD CONSTRAINT record_blob_pkey PRIMARY KEY ("blobCid","recordUri");

-- Create Backlink Table
CREATE TABLE IF NOT EXISTS pds.backlink (
    uri character varying NOT NULL,
    path character varying NOT NULL,
    "linkTo" character varying NOT NULL
);
ALTER TABLE ONLY pds.backlink
    DROP CONSTRAINT IF EXISTS backlink_pkey;
ALTER TABLE ONLY pds.backlink
    ADD CONSTRAINT backlink_pkey PRIMARY KEY (uri, path);
CREATE INDEX backlink_link_to_idx
	ON pds.backlink(path, "linkTo");

-- Create Account Preferences Table
CREATE TABLE IF NOT EXISTS pds.account_pref (
	id SERIAL PRIMARY KEY,
    did character varying NOT NULL,
    name character varying NOT NULL,
    "valueJson" text
);

-- did-cache implementation
-- Create DID Cache Table
CREATE TABLE IF NOT EXISTS pds.did_doc (
    did character varying PRIMARY KEY,
    doc text NOT NULL,
    "updatedAt" bigint NOT NULL
);

-- sequencer implementation
-- Create Repo Sequence Table
CREATE TABLE IF NOT EXISTS pds.repo_seq (
    seq bigserial PRIMARY KEY,
    did character varying NOT NULL,
    "eventType" character varying NOT NULL,
    event bytea NOT NULL,
    invalidated smallint NOT NULL DEFAULT 0,
    "sequencedAt" character varying NOT NULL
);
CREATE INDEX repo_seq_did_idx -- for filtering seqs based on did
	ON pds.repo_seq(did);
CREATE INDEX repo_seq_event_type_idx -- for filtering seqs based on event type
	ON pds.repo_seq("eventType");
CREATE INDEX repo_seq_sequenced_at_index -- for entering into the seq stream at a particular time
	ON pds.repo_seq("sequencedAt");