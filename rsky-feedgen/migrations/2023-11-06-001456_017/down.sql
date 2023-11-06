-- This file should undo anything in `up.sql`
ALTER TABLE post 
	ADD CONSTRAINT unique_sequence UNIQUE (sequence);
ALTER TABLE public.like 
	ADD CONSTRAINT unique_like_sequence UNIQUE (sequence);
ALTER TABLE ONLY public.follow 
	ADD CONSTRAINT unique_follow_sequence UNIQUE (sequence);