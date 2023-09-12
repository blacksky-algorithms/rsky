-- Your SQL goes here
CREATE TABLE IF NOT EXISTS public.image (
    cid character varying NOT NULL,
    alt character varying,
    "postCid" character varying NOT NULL,
    "postUri" character varying NOT NULL,
    "createdAt" character varying NOT NULL,
    "indexedAt" character varying NOT NULL
);

ALTER TABLE ONLY public.image
    DROP CONSTRAINT IF EXISTS image_pkey;
ALTER TABLE ONLY public.image
    ADD CONSTRAINT image_pkey PRIMARY KEY (cid);