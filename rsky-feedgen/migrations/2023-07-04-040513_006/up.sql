-- Your SQL goes here
CREATE TABLE IF NOT EXISTS public.visitor (
	id SERIAL PRIMARY KEY,
    did character varying NOT NULL,
    web character varying NOT NULL,
    visited_at character varying NOT NULL
);