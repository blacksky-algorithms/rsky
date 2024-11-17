-- For tracking people not allowed to access a feed
CREATE TABLE IF NOT EXISTS public.banned_from_tv (
    did character varying NOT NULL,
    reason character varying,
    "createdAt" character varying DEFAULT to_char(now(), 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"'),
    tags TEXT []
);

ALTER TABLE ONLY public.banned_from_tv
    DROP CONSTRAINT IF EXISTS banned_from_tv_pkey;
ALTER TABLE ONLY public.banned_from_tv
    ADD CONSTRAINT banned_from_tv_pkey PRIMARY KEY (did);