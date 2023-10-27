-- This file should undo anything in `up.sql`
ALTER TABLE public.membership 
DROP CONSTRAINT IF EXISTS membership_pkey;

ALTER TABLE ONLY public.membership
    ADD CONSTRAINT membership_pkey PRIMARY KEY (did);