-- This file should undo anything in `up.sql`
DROP TABLE pds.repo_seq;
DROP TABLE pds.did_doc;
DROP TABLE pds.account_pref;
DROP TABLE pds.backlink;
DROP TABLE pds.record_blob;
DROP TABLE pds.blob;
DROP TABLE pds.record;
DROP TABLE pds.repo_block;
DROP TABLE pds.repo_root;
DROP TABLE pds.email_token;
DROP TABLE pds.account;
DROP TABLE pds.actor;
DROP TABLE pds.refresh_token;
DROP TABLE pds.invite_code_use;
DROP TABLE pds.invite_code;
DROP TABLE pds.app_password;
DROP SCHEMA IF EXISTS pds;
