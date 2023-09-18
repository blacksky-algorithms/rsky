-- This file should undo anything in `up.sql`
ALTER TABLE public.image 
DROP COLUMN IF EXISTS labels;