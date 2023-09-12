-- This file should undo anything in `up.sql`
ALTER TABLE public.post 
DROP COLUMN IF EXISTS "text",
DROP COLUMN IF EXISTS lang;