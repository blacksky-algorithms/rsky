CREATE INDEX post_text_tsvector_index ON public.post USING GIN (to_tsvector('simple', text));

