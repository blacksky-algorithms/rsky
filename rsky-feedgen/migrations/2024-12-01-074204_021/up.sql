-- Your SQL goes here
CREATE INDEX idx_like_subjecturi_indexedat ON public.like ("subjectUri", "indexedAt");
CREATE INDEX idx_post_createdAt_cid ON "post" ("createdAt" DESC, "cid" DESC)
    WHERE "replyParent" IS NULL AND "replyRoot" IS NULL;