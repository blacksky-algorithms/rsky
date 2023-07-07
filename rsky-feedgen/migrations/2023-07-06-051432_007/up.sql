-- Your SQL goes here
CREATE TABLE IF NOT EXISTS public.like (
    uri character varying NOT NULL,
    cid character varying NOT NULL,
    author character varying NOT NULL,
    "subjectCid" character varying NOT NULL,
    "subjectUri" character varying NOT NULL,
    "createdAt" character varying NOT NULL,
    "indexedAt" character varying NOT NULL
);

ALTER TABLE ONLY public.like
    DROP CONSTRAINT IF EXISTS like_pkey;
ALTER TABLE ONLY public.like
    ADD CONSTRAINT like_pkey PRIMARY KEY (uri);