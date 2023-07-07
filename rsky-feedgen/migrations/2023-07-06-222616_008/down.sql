-- This file should undo anything in `up.sql`
ALTER TABLE public.like 
DROP CONSTRAINT IF EXISTS unique_like_sequence;

ALTER TABLE public.like 
DROP COLUMN IF EXISTS prev,
DROP COLUMN IF EXISTS sequence;