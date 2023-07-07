-- Your SQL goes here
ALTER TABLE public.like 
ADD COLUMN prev VARCHAR,
ADD COLUMN sequence NUMERIC;

ALTER TABLE public.like 
ADD CONSTRAINT unique_like_sequence UNIQUE (sequence);