-- Your SQL goes here
CREATE TABLE IF NOT EXISTS public.follow (
    uri character varying NOT NULL,
    cid character varying NOT NULL,
    author character varying NOT NULL,
    "subject" character varying NOT NULL,
    "createdAt" character varying NOT NULL,
    "indexedAt" character varying NOT NULL,
    prev character varying,
    sequence bigint
);

ALTER TABLE ONLY public.follow
    DROP CONSTRAINT IF EXISTS follow_pkey;
ALTER TABLE ONLY public.follow
    ADD CONSTRAINT follow_pkey PRIMARY KEY (uri);
ALTER TABLE ONLY public.follow
    DROP CONSTRAINT IF EXISTS unique_follow_sequence;
ALTER TABLE ONLY public.follow 
	ADD CONSTRAINT unique_follow_sequence UNIQUE (sequence);