-- Your SQL goes here
ALTER TABLE public.membership 
DROP CONSTRAINT IF EXISTS membership_pkey;

ALTER TABLE public.membership 
ADD CONSTRAINT membership_pkey PRIMARY KEY (did, list);