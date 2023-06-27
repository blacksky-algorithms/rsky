-- Your SQL goes here
ALTER TABLE post 
ADD COLUMN prev VARCHAR,
ADD COLUMN sequence NUMERIC;

ALTER TABLE post 
ADD CONSTRAINT unique_sequence UNIQUE (sequence);
