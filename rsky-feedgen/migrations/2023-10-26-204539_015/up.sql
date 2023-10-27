-- Your SQL goes here
ALTER TABLE post 
ADD COLUMN author character varying NOT NULL DEFAULT '',
ADD COLUMN "externalUri" character varying,
ADD COLUMN "externalTitle" character varying,
ADD COLUMN "externalDescription" character varying,
ADD COLUMN "externalThumb" character varying,
ADD COLUMN "quoteCid" character varying,
ADD COLUMN "quoteUri" character varying;