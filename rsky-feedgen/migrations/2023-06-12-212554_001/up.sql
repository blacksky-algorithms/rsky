-- Your SQL goes here
CREATE TABLE IF NOT EXISTS public.post (
    uri character varying NOT NULL,
    cid character varying NOT NULL,
    "replyParent" character varying,
    "replyRoot" character varying,
    "indexedAt" character varying NOT NULL
);

ALTER TABLE ONLY public.post
    DROP CONSTRAINT IF EXISTS post_pkey;
ALTER TABLE ONLY public.post
    ADD CONSTRAINT post_pkey PRIMARY KEY (uri);

CREATE TABLE IF NOT EXISTS public.sub_state (
    service character varying NOT NULL,
    cursor integer NOT NULL
);

ALTER TABLE ONLY public.sub_state
    DROP CONSTRAINT IF EXISTS sub_state_pkey;
ALTER TABLE ONLY public.sub_state
    ADD CONSTRAINT sub_state_pkey PRIMARY KEY (service);