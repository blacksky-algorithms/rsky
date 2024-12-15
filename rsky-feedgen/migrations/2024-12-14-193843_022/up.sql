-- Your SQL goes here
ALTER TABLE post
ADD COLUMN labels TEXT [] NOT NULL DEFAULT '{}',
ADD CONSTRAINT no_nulls_in_labels CHECK (NOT (labels && ARRAY[NULL]));