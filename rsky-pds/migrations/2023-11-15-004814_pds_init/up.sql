-- Create PDS schema
CREATE SCHEMA IF NOT EXISTS pds;

-- Create App Migration Table
CREATE TABLE IF NOT EXISTS pds.app_migration (
    id character varying PRIMARY KEY,
    success smallint NOT NULL DEFAULT 0,
    "completedAt" character varying
);

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

-- Create Backlink Table
CREATE TABLE IF NOT EXISTS pds.backlink (
    uri character varying NOT NULL,
    "path" character varying NOT NULL,
    "linkToUri" character varying,
    "linkToDid" character varying
);

ALTER TABLE ONLY pds.backlink
    DROP CONSTRAINT IF EXISTS backlink_pkey;
ALTER TABLE ONLY pds.backlink
    ADD CONSTRAINT backlink_pkey PRIMARY KEY (uri, path);
ALTER TABLE ONLY pds.backlink
    DROP CONSTRAINT IF EXISTS backlink_link_to_chk;
-- Exactly one of linkToUri or linkToDid should be set
ALTER TABLE ONLY pds.backlink 
	ADD CONSTRAINT backlink_link_to_chk 
	CHECK (
		("linkToUri" is null and "linkToDid" is not null)
		OR ("linkToUri" is not null and "linkToDid" is null)
	);

CREATE INDEX backlink_path_to_uri_idx 
	ON pds.backlink(path, "linkToUri");
CREATE INDEX backlink_path_to_did_idx 
	ON pds.backlink(path, "linkToDid");

-- Create Blob Table
CREATE TABLE IF NOT EXISTS pds.blob (
    creator character varying NOT NULL,
    cid character varying NOT NULL,
    "mimeType" character varying NOT NULL,
    size integer NOT NULL,
    "tempKey" character varying,
    width integer,
    height integer,
    "createdAt" character varying NOT NULL
);

ALTER TABLE ONLY pds.blob
    DROP CONSTRAINT IF EXISTS blob_pkey;
ALTER TABLE ONLY pds.blob
    ADD CONSTRAINT blob_pkey PRIMARY KEY (creator, cid);
CREATE INDEX blob_tempkey_idx 
	ON pds.blob("tempKey");

-- Create DID Cache Table
CREATE TABLE IF NOT EXISTS pds.did_cache (
    did character varying PRIMARY KEY,
    doc text NOT NULL,
    "updatedAt" bigint NOT NULL
);

-- Create DID Handle Table
CREATE TABLE IF NOT EXISTS pds.did_handle (
    did character varying PRIMARY KEY,
    handle character varying
);
CREATE UNIQUE INDEX did_handle_handle_lower_idx 
	ON pds.did_handle (LOWER(handle));

-- Create Invite Code Table
CREATE TABLE IF NOT EXISTS pds.invite_code (
    code character varying PRIMARY KEY,
    "availableUses" integer NOT NULL,
    disabled smallint NOT NULL DEFAULT 0,
    "forUser" character varying NOT NULL,
    "createdBy" character varying NOT NULL,
    "createdAt" character varying NOT NULL
);
CREATE INDEX invite_code_for_user_idx 
	ON pds.invite_code("forUser");

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

-- Create IPLD Block Table
CREATE TABLE IF NOT EXISTS pds.ipld_block (
    creator character varying NOT NULL,
    cid character varying NOT NULL,
    size integer NOT NULL,
    content bytea NOT NULL,
    "repoRev" character varying
);

ALTER TABLE ONLY pds.ipld_block
    DROP CONSTRAINT IF EXISTS ipld_block_pkey;
ALTER TABLE ONLY pds.ipld_block
    ADD CONSTRAINT ipld_block_pkey PRIMARY KEY (creator, cid);
CREATE INDEX ipld_block_repo_rev_idx 
	ON pds.ipld_block(creator, "repoRev", cid);

-- Create Moderation Action Table
CREATE TABLE IF NOT EXISTS pds.moderation_action (
	id SERIAL PRIMARY KEY,
    action character varying NOT NULL,
    "subjectType" character varying NOT NULL,
    "subjectDid" character varying NOT NULL,
    "subjectUri" character varying,
    "subjectCid" character varying,
    reason text NOT NULL,
    "createdAt" character varying NOT NULL,
    "createdBy" character varying NOT NULL,
    "reversedAt" character varying,
    "reversedBy" character varying,
    "reversedReason" text,
    "createLabelVals" character varying,
    "negateLabelVals" character varying,
    "durationInHours" integer,
    "expiresAt" character varying
);

-- Create Moderation Action Subject Blob Table
CREATE TABLE IF NOT EXISTS pds.moderation_action_subject_blob (
	id SERIAL PRIMARY KEY,
    "actionId" integer NOT NULL,
    cid character varying NOT NULL,
    "recordUri" character varying NOT NULL,
	CONSTRAINT fk_subject_action
		FOREIGN KEY("actionId") 
			REFERENCES pds.moderation_action(id)
);
ALTER TABLE ONLY pds.moderation_action_subject_blob
    DROP CONSTRAINT IF EXISTS moderation_action_subject_blob_pkey;
ALTER TABLE ONLY pds.moderation_action_subject_blob
    ADD CONSTRAINT moderation_action_subject_blob_pkey PRIMARY KEY ("actionId", cid, "recordUri");

-- Create Moderation Report Table
CREATE TABLE IF NOT EXISTS pds.moderation_report (
	id SERIAL PRIMARY KEY,
    "subjectType" character varying NOT NULL,
    "subjectDid" character varying NOT NULL,
    "subjectUri" character varying,
    "subjectCid" character varying,
    "reasonType" character varying NOT NULL,
    reason text,
    "reportedByDid" character varying NOT NULL,
    "createdAt" character varying NOT NULL
);

-- Create Moderation Report Resolution Table
CREATE TABLE IF NOT EXISTS pds.moderation_report_resolution (
    "reportId" integer NOT NULL,
    "actionId" integer NOT NULL,
    "createdAt" character varying NOT NULL,
    "createdBy" character varying NOT NULL,
	CONSTRAINT fk_report_resolution
		FOREIGN KEY("reportId") 
			REFERENCES pds.moderation_report(id),
	CONSTRAINT fk_action_resolution
		FOREIGN KEY("actionId") 
			REFERENCES pds.moderation_action(id)
);
ALTER TABLE ONLY pds.moderation_report_resolution
    DROP CONSTRAINT IF EXISTS moderation_report_resolution_pkey;
ALTER TABLE ONLY pds.moderation_report_resolution
    ADD CONSTRAINT moderation_report_resolution_pkey PRIMARY KEY ("reportId","actionId");

CREATE INDEX moderation_report_resolution_action_id_idx 
	ON pds.moderation_report_resolution("actionId");

-- Create Record Table
CREATE TABLE IF NOT EXISTS pds.record (
    uri character varying PRIMARY KEY,
    cid character varying NOT NULL,
    did character varying NOT NULL,
    collection character varying NOT NULL,
    "rkey" character varying NOT NULL,
    "indexedAt" character varying NOT NULL,
    "takedownRef" character varying,
    "repoRev" character varying
);
CREATE INDEX record_did_cid_idx 
	ON pds.record(did, cid);
CREATE INDEX record_did_collection_idx 
	ON pds.record(did, collection);
CREATE INDEX record_repo_rev_idx 
	ON pds.record(did, "repoRev");

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

-- Create Repo Blob Table
CREATE TABLE IF NOT EXISTS pds.repo_blob (
    cid character varying NOT NULL,
    "recordUri" character varying NOT NULL,
    did character varying NOT NULL,
    "takedownRef" character varying,
    "repoRev" character varying
);
ALTER TABLE ONLY pds.repo_blob
    DROP CONSTRAINT IF EXISTS repo_blob_pkey;
ALTER TABLE ONLY pds.repo_blob
    ADD CONSTRAINT repo_blob_pkey PRIMARY KEY (cid,"recordUri");

CREATE INDEX repo_blob_did_idx 
	ON pds.repo_blob(did);
CREATE INDEX repo_blob_repo_rev_idx 
	ON pds.repo_blob(did, "repoRev");

-- Create Repo Root Table
CREATE TABLE IF NOT EXISTS pds.repo_root (
    did character varying PRIMARY KEY,
    root character varying NOT NULL,
    "indexedAt" character varying NOT NULL,
    "takedownRef" character varying,
    rev character varying
);

-- Create Repo Sequence Table
CREATE TABLE IF NOT EXISTS pds.repo_seq (
    id bigserial PRIMARY KEY,
    seq bigint UNIQUE,
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

-- Create User Account Table
CREATE TABLE IF NOT EXISTS pds.user_account (
    did character varying PRIMARY KEY,
    email character varying NOT NULL,
    "recoveryKey" character varying,
    "password" character varying NOT NULL,
    "createdAt" character varying NOT NULL,
	"invitesDisabled" smallint NOT NULL DEFAULT 0,
	"inviteNote" character varying,
	"emailConfirmedAt" character varying
);
CREATE UNIQUE INDEX user_account_email_lower_idx 
	ON pds.user_account (LOWER(email));
CREATE INDEX user_account_cursor_idx
	ON pds.user_account("createdAt", did);

-- Create User Preference Table
CREATE TABLE IF NOT EXISTS pds.user_pref (
    id bigserial PRIMARY KEY,
    did character varying NOT NULL,
    name character varying NOT NULL,
    "valueJson" text NOT NULL
);
CREATE INDEX user_pref_did_idx
	ON pds.user_pref(did);

-- Create Runtime Flag Table
CREATE TABLE IF NOT EXISTS pds.runtime_flag (
    name character varying PRIMARY KEY,
    value character varying NOT NULL
);

-- Create Email Token Table
CREATE TABLE IF NOT EXISTS pds.email_token (
    purpose character varying NOT NULL,
    did character varying NOT NULL,
    token character varying NOT NULL,
    "requestedAt" timestamptz NOT NULL
);
ALTER TABLE ONLY pds.email_token
    DROP CONSTRAINT IF EXISTS email_token_pkey;
ALTER TABLE ONLY pds.email_token
    ADD CONSTRAINT email_token_pkey PRIMARY KEY (purpose, did);
CREATE UNIQUE INDEX email_token_purpose_token_unique 
	ON pds.email_token (purpose, token);
