-- Your SQL goes here
ALTER TABLE pds.actor
    ADD COLUMN "deactivatedAt" character varying,
    ADD COLUMN "deleteAfter" character varying;