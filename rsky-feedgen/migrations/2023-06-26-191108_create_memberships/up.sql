-- Your SQL goes here
CREATE TABLE IF NOT EXISTS public.membership (
    did character varying NOT NULL,
    included BOOLEAN NOT NULL,
    excluded BOOLEAN NOT NULL,
    list character varying NOT NULL
);

ALTER TABLE ONLY public.membership
    ADD CONSTRAINT membership_pkey PRIMARY KEY (did);