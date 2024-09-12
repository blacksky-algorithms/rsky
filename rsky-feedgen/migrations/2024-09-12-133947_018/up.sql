-- Your SQL goes here
CREATE TABLE IF NOT EXISTS public.video (
    cid character varying NOT NULL,
    alt character varying,
    "postCid" character varying NOT NULL,
    "postUri" character varying NOT NULL,
    "createdAt" character varying NOT NULL,
    "indexedAt" character varying NOT NULL,
    labels TEXT []
);

ALTER TABLE ONLY public.video
    DROP CONSTRAINT IF EXISTS video_pkey;
ALTER TABLE ONLY public.video
    ADD CONSTRAINT video_pkey PRIMARY KEY (cid);