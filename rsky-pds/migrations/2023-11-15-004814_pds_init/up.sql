-- Create App Migration Table
CREATE TABLE IF NOT EXISTS public.app_migration (
    id character varying NOT NULL,
    success smallint NOT NULL DEFAULT 0,
    "completedAt" character varying
);

ALTER TABLE ONLY public.app_migration
    DROP CONSTRAINT IF EXISTS app_migration_pkey;
ALTER TABLE ONLY public.app_migration
    ADD CONSTRAINT app_migration_pkey PRIMARY KEY (id);

-- Create App Password Table
CREATE TABLE IF NOT EXISTS public.app_password (
    did character varying NOT NULL,
    name character varying NOT NULL,
    "passwordScrypt" character varying NOT NULL,
    "createdAt" character varying NOT NULL
);

ALTER TABLE ONLY public.app_password
    DROP CONSTRAINT IF EXISTS app_password_pkey;
ALTER TABLE ONLY public.app_password
    ADD CONSTRAINT app_password_pkey PRIMARY KEY (did, name);

-- Create App Password Table
CREATE TABLE IF NOT EXISTS public.backlink (
    uri character varying NOT NULL,
    "path" character varying NOT NULL,
    "linkToUri" character varying,
    "linkToDid" character varying
);

ALTER TABLE ONLY public.backlink
    DROP CONSTRAINT IF EXISTS backlink_pkey;
ALTER TABLE ONLY public.backlink
    ADD CONSTRAINT backlink_pkey PRIMARY KEY (uri, path);
ALTER TABLE ONLY public.backlink
    DROP CONSTRAINT IF EXISTS backlink_link_to_chk;
-- Exactly one of linkToUri or linkToDid should be set
ALTER TABLE ONLY public.backlink 
	ADD CONSTRAINT backlink_link_to_chk 
	CHECK (
		("linkToUri" is null and "linkToDid" is not null)
		OR ("linkToUri" is not null and "linkToDid" is null)
	);

CREATE INDEX backlink_path_to_uri_idx 
	ON public.backlink(path, "linkToUri");
CREATE INDEX backlink_path_to_did_idx 
	ON public.backlink(path, "linkToDid");

-- Create Blob Table
CREATE TABLE IF NOT EXISTS public.blob (
    creator character varying NOT NULL,
    cid character varying NOT NULL,
    "mimeType" character varying NOT NULL,
    size integer NOT NULL,
    "tempKey" character varying,
    width integer,
    height integer,
    "createdAt" character varying NOT NULL
);

ALTER TABLE ONLY public.blob
    DROP CONSTRAINT IF EXISTS blob_pkey;
ALTER TABLE ONLY public.blob
    ADD CONSTRAINT blob_pkey PRIMARY KEY (creator, cid);

-- Create Delete Account Token Table
CREATE TABLE IF NOT EXISTS public.delete_account_token (
    did character varying NOT NULL,
    token character varying NOT NULL,
    "requestedAt" character varying NOT NULL
);

ALTER TABLE ONLY public.delete_account_token
    DROP CONSTRAINT IF EXISTS delete_account_token_pkey;
ALTER TABLE ONLY public.delete_account_token
    ADD CONSTRAINT delete_account_token_pkey PRIMARY KEY (did);

-- Create DID Cache Table
CREATE TABLE IF NOT EXISTS public.did_cache (
    did character varying NOT NULL,
    doc text NOT NULL,
    "updatedAt" bigint NOT NULL
);

ALTER TABLE ONLY public.did_cache
    DROP CONSTRAINT IF EXISTS did_cache_pkey;
ALTER TABLE ONLY public.did_cache
    ADD CONSTRAINT did_cache_pkey PRIMARY KEY (did);
