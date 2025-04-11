CREATE TABLE IF NOT EXISTS pds.authorization_request
(
    id           character varying NOT NULL,
    did          character varying,
    "deviceId"   character varying,
    "clientId"   character varying NOT NULL,
    "clientAuth" character varying NOT NULL,
    parameters   character varying NOT NULL,
    "expiresAt"  bigint            NOT NULL,
    code         character varying
);

ALTER TABLE ONLY pds.authorization_request
    DROP CONSTRAINT IF EXISTS authorization_request_code_idx;
ALTER TABLE ONLY pds.authorization_request
    ADD CONSTRAINT authorization_request_code_idx PRIMARY KEY (id);

-- TODO expires at index

CREATE TABLE IF NOT EXISTS pds.device
(
    id           character varying NOT NULL,
    "sessionId"  character varying unique,
    "userAgent"  character varying,
    "ipAddress"  character varying NOT NULL,
    "lastSeenAt" character varying NOT NULL
);

ALTER TABLE ONLY pds.device
    DROP CONSTRAINT IF EXISTS pds_idx;
ALTER TABLE ONLY pds.device
    ADD CONSTRAINT pds_idx PRIMARY KEY (id);

CREATE TABLE IF NOT EXISTS pds.device_account
(
    did                 character varying NOT NULL,
    "deviceId"          character varying NOT NULL,
    "authenticatedAt"   character varying NOT NULL,
    remember            boolean           NOT NULL,
    "authorizedClients" character varying NOT NULL
);

ALTER TABLE ONLY pds.device_account
    DROP CONSTRAINT IF EXISTS device_account_pk;
ALTER TABLE ONLY pds.device_account
    ADD CONSTRAINT device_account_pk PRIMARY KEY ("deviceId", did);

--TODO add foreign key constraints

CREATE TABLE IF NOT EXISTS pds.token
(
    id                    character varying NOT NULL, --TODO
    did                   character varying NOT NULL,
    "tokenId"             character varying NOT NULL unique,
    "createdAt"           boolean           NOT NULL,
    "updatedAt"           character varying NOT NULL,
    "expiresAt"           bigint            NOT NULL,
    "clientId"            character varying NOT NULL,
    "clientAuth"          character varying NOT NULL,
    "deviceId"            character varying,
    parameters            character varying NOT NULL,
    details               character varying,
    code                  character varying,
    "currentRefreshToken" character varying unique
);

ALTER TABLE ONLY pds.token
    DROP CONSTRAINT IF EXISTS token_idx;
ALTER TABLE ONLY pds.token
    ADD CONSTRAINT token_idx PRIMARY KEY (id);

CREATE TABLE IF NOT EXISTS pds.used_refresh_token
(
    "refreshToken" character varying NOT NULL,
    "tokenId"      character varying NOT NULL
);

ALTER TABLE ONLY pds.used_refresh_token
    DROP CONSTRAINT IF EXISTS used_refresh_token_pk;
ALTER TABLE ONLY pds.used_refresh_token
    ADD CONSTRAINT used_refresh_token_pk PRIMARY KEY ("refreshToken");



