-- This file should undo anything in `up.sql`
ALTER TABLE public.visitor 
DROP COLUMN IF EXISTS feed;