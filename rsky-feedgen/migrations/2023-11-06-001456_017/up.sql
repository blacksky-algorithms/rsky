-- Your SQL goes here
ALTER TABLE post 
	DROP CONSTRAINT IF EXISTS unique_sequence;
ALTER TABLE public.like  
	DROP CONSTRAINT IF EXISTS unique_like_sequence;
ALTER TABLE ONLY public.follow
    DROP CONSTRAINT IF EXISTS unique_follow_sequence;