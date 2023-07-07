-- This file should undo anything in `up.sql`
ALTER TABLE public.like 
ALTER COLUMN sequence TYPE NUMERIC;