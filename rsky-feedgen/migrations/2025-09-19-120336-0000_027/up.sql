-- !no-transaction
-- For membership filtering
CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_membership_list_included ON membership(list, included, did);

-- For like author lookup
CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_like_author_indexed ON public.like(author, "indexedAt", "subjectUri");

-- For quote post counting
CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_post_quoteuri_indexed ON post("quoteUri", "indexedAt") WHERE "quoteUri" IS NOT NULL;