-- Bluesky appview dataplane schema, for rsky-wintermute's DB-backed tests.
--
-- Provenance: pg_dump --schema-only --no-owner --no-privileges --no-comments -n bsky
-- of the appview database on the wintermute-profiling box (PostgreSQL 17.11),
-- taken 2026-09-07 at kysely migration _20260816T120000000Z (71 migrations of the
-- blacksky-algorithms/atproto fork, branch main). Structure only: tables, generated
-- columns, defaults, sequences, primary keys, unique/foreign-key constraints,
-- indexes, the notification push trigger and the algo_whats_hot materialized view.
-- No rows (kysely_migration is empty on purpose).
--
-- Production runs with search_path=bsky; here every `bsky.` qualifier has been
-- stripped so the objects land in `public` and the tests' default
-- DATABASE_URL (postgresql://postgres:postgres@localhost:5432/bsky_test) needs
-- no search_path option. wintermute's own queries are unqualified, so the two
-- layouts behave identically. See rsky-wintermute/README.md ("Running the
-- database-backed tests") for how to apply and regenerate this file.

-- The actor/profile/starter_pack trigram indexes need pg_trgm (bundled with the
-- stock postgres image).
CREATE EXTENSION IF NOT EXISTS pg_trgm;

--
-- Name: notify_notification_push_insert(); Type: FUNCTION
--

CREATE FUNCTION notify_notification_push_insert() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
    begin
      perform pg_notify(
        'notification_push_inserted',
        json_build_object(
          'id', new.id,
          'did', new.did,
          'recordUri', new."recordUri",
          'reason', new.reason,
          'reasonSubject', new."reasonSubject"
        )::text
      );
      return new;
    end;
    $$;

--
-- Name: activity_subscription; Type: TABLE
--

CREATE TABLE activity_subscription (
    creator character varying NOT NULL,
    "subjectDid" character varying NOT NULL,
    key character varying NOT NULL,
    "indexedAt" character varying NOT NULL,
    post boolean NOT NULL,
    reply boolean NOT NULL
);

--
-- Name: actor; Type: TABLE
--

CREATE TABLE actor (
    did character varying NOT NULL,
    handle character varying,
    "indexedAt" character varying NOT NULL,
    "takedownRef" character varying,
    "upstreamStatus" character varying,
    "trustedVerifier" boolean DEFAULT false NOT NULL,
    "ageAssuranceStatus" text,
    "ageAssuranceLastInitiatedAt" character varying,
    "ageAssuranceAccess" text,
    "ageAssuranceCountryCode" text,
    "ageAssuranceRegionCode" text,
    "handleResolveTries" smallint DEFAULT 0 NOT NULL,
    "accountEventAt" timestamp with time zone
);

--
-- Name: actor_badge; Type: TABLE
--

CREATE TABLE actor_badge (
    id bigint NOT NULL,
    did character varying NOT NULL,
    badge character varying NOT NULL,
    "issuedBy" character varying NOT NULL,
    "createdAt" timestamp with time zone DEFAULT now() NOT NULL,
    "revokedAt" timestamp with time zone,
    "revokedBy" character varying
);

--
-- Name: actor_badge_id_seq; Type: SEQUENCE
--

CREATE SEQUENCE actor_badge_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

--
-- Name: actor_badge_id_seq; Type: SEQUENCE OWNED BY
--

ALTER SEQUENCE actor_badge_id_seq OWNED BY actor_badge.id;

--
-- Name: actor_block; Type: TABLE
--

CREATE TABLE actor_block (
    uri character varying NOT NULL,
    cid character varying NOT NULL,
    creator character varying NOT NULL,
    "subjectDid" character varying NOT NULL,
    "createdAt" character varying NOT NULL,
    "indexedAt" character varying NOT NULL,
    "sortAt" character varying GENERATED ALWAYS AS (LEAST("createdAt", "indexedAt")) STORED NOT NULL,
    "takedownRef" character varying
);

--
-- Name: actor_state; Type: TABLE
--

CREATE TABLE actor_state (
    did character varying NOT NULL,
    "lastSeenNotifs" character varying NOT NULL,
    "priorityNotifs" boolean DEFAULT false NOT NULL,
    "lastSeenPriorityNotifs" character varying
);

--
-- Name: actor_sync; Type: TABLE
--

CREATE TABLE actor_sync (
    did character varying NOT NULL,
    "commitCid" character varying NOT NULL,
    "repoRev" character varying
);

--
-- Name: post; Type: TABLE
--

CREATE TABLE post (
    uri character varying NOT NULL,
    cid character varying NOT NULL,
    creator character varying NOT NULL,
    text character varying NOT NULL,
    "replyRoot" character varying,
    "replyRootCid" character varying,
    "replyParent" character varying,
    "replyParentCid" character varying,
    "createdAt" character varying NOT NULL,
    "indexedAt" character varying NOT NULL,
    "sortAt" character varying GENERATED ALWAYS AS (LEAST("createdAt", "indexedAt")) STORED NOT NULL,
    langs character varying[],
    "invalidReplyRoot" boolean,
    "violatesThreadGate" boolean,
    tags character varying[],
    "violatesEmbeddingRules" boolean,
    "hasThreadGate" boolean,
    "hasPostGate" boolean
);

--
-- Name: post_agg; Type: TABLE
--

CREATE TABLE post_agg (
    uri character varying NOT NULL,
    "likeCount" bigint DEFAULT 0 NOT NULL,
    "replyCount" bigint DEFAULT 0 NOT NULL,
    "repostCount" bigint DEFAULT 0 NOT NULL,
    "quoteCount" bigint DEFAULT 0 NOT NULL,
    "bookmarkCount" bigint DEFAULT 0 NOT NULL
);

--
-- Name: view_param; Type: TABLE
--

CREATE TABLE view_param (
    name character varying NOT NULL,
    value character varying
);

--
-- Name: algo_whats_hot_view; Type: MATERIALIZED VIEW
--

CREATE MATERIALIZED VIEW algo_whats_hot_view AS
 SELECT post.uri,
    post.cid,
    round(((1000000)::numeric * ((post_agg."likeCount")::numeric / (((EXTRACT(epoch FROM age(now(), ((post."indexedAt")::timestamp without time zone)::timestamp with time zone)) / (3600)::numeric) + (2)::numeric) ^ 1.8)))) AS score
   FROM (post
     JOIN post_agg ON (((post_agg.uri)::text = (post.uri)::text)))
  WHERE (((post."indexedAt")::text > ( SELECT to_char((now() - (view_param.value)::interval), 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"'::text) AS val
           FROM view_param
          WHERE ((view_param.name)::text = 'whats_hot_interval'::text))) AND (post."replyParent" IS NULL) AND (post_agg."likeCount" > ( SELECT (view_param.value)::integer AS val
           FROM view_param
          WHERE ((view_param.name)::text = 'whats_hot_like_threshold'::text))))
  WITH NO DATA;

--
-- Name: blob_takedown; Type: TABLE
--

CREATE TABLE blob_takedown (
    did character varying NOT NULL,
    cid character varying NOT NULL,
    "takedownRef" character varying NOT NULL
);

--
-- Name: bookmark; Type: TABLE
--

CREATE TABLE bookmark (
    creator character varying NOT NULL,
    key character varying NOT NULL,
    "subjectUri" character varying NOT NULL,
    "subjectCid" character varying NOT NULL,
    "indexedAt" character varying NOT NULL
);

--
-- Name: community_post; Type: TABLE
--

CREATE TABLE community_post (
    uri character varying NOT NULL,
    cid character varying DEFAULT ''::character varying NOT NULL,
    rkey character varying NOT NULL,
    creator character varying NOT NULL,
    text text DEFAULT ''::text NOT NULL,
    facets jsonb,
    "replyRoot" character varying,
    "replyRootCid" character varying,
    "replyParent" character varying,
    "replyParentCid" character varying,
    embed jsonb,
    langs character varying,
    labels jsonb,
    tags character varying,
    "createdAt" character varying NOT NULL,
    "indexedAt" character varying NOT NULL,
    "sortAt" character varying GENERATED ALWAYS AS (LEAST("createdAt", "indexedAt")) STORED NOT NULL,
    "threadgateAllow" jsonb,
    "embeddingRules" jsonb,
    space_uri character varying,
    moderation_flagged_at character varying,
    moderation_flagged_by character varying,
    projection_revision character varying
);

--
-- Name: did_cache; Type: TABLE
--

CREATE TABLE did_cache (
    did character varying NOT NULL,
    doc jsonb NOT NULL,
    "updatedAt" bigint NOT NULL
);

--
-- Name: draft; Type: TABLE
--

CREATE TABLE draft (
    creator character varying NOT NULL,
    key character varying NOT NULL,
    "createdAt" character varying NOT NULL,
    "updatedAt" character varying NOT NULL,
    payload text NOT NULL
);

--
-- Name: duplicate_record; Type: TABLE
--

CREATE TABLE duplicate_record (
    uri character varying NOT NULL,
    cid character varying NOT NULL,
    "duplicateOf" character varying NOT NULL,
    "indexedAt" character varying NOT NULL
);

--
-- Name: feed_generator; Type: TABLE
--

CREATE TABLE feed_generator (
    uri character varying NOT NULL,
    cid character varying NOT NULL,
    creator character varying NOT NULL,
    "feedDid" character varying NOT NULL,
    "displayName" character varying,
    description character varying,
    "descriptionFacets" character varying,
    "avatarCid" character varying,
    "createdAt" character varying NOT NULL,
    "indexedAt" character varying NOT NULL,
    "sortAt" character varying GENERATED ALWAYS AS (LEAST("createdAt", "indexedAt")) STORED NOT NULL
);

--
-- Name: feed_item; Type: TABLE
--

CREATE TABLE feed_item (
    uri character varying NOT NULL,
    cid character varying NOT NULL,
    type character varying NOT NULL,
    "postUri" character varying NOT NULL,
    "originatorDid" character varying NOT NULL,
    "sortAt" character varying NOT NULL
);

--
-- Name: follow; Type: TABLE
--

CREATE TABLE follow (
    uri character varying NOT NULL,
    cid character varying NOT NULL,
    creator character varying NOT NULL,
    "subjectDid" character varying NOT NULL,
    "createdAt" character varying NOT NULL,
    "indexedAt" character varying NOT NULL,
    "sortAt" character varying GENERATED ALWAYS AS (LEAST("createdAt", "indexedAt")) STORED NOT NULL,
    via character varying,
    "viaCid" character varying,
    "takedownRef" character varying
);

--
-- Name: kysely_migration; Type: TABLE
--

CREATE TABLE kysely_migration (
    name character varying(255) NOT NULL,
    "timestamp" character varying(255) NOT NULL
);

--
-- Name: kysely_migration_lock; Type: TABLE
--

CREATE TABLE kysely_migration_lock (
    id character varying(255) NOT NULL,
    is_locked integer DEFAULT 0 NOT NULL
);

--
-- Name: label; Type: TABLE
--

CREATE TABLE label (
    src character varying NOT NULL,
    uri character varying NOT NULL,
    cid character varying NOT NULL,
    val character varying NOT NULL,
    neg boolean NOT NULL,
    cts character varying NOT NULL,
    exp character varying
);

--
-- Name: labeler; Type: TABLE
--

CREATE TABLE labeler (
    uri character varying NOT NULL,
    cid character varying NOT NULL,
    creator character varying NOT NULL,
    "createdAt" character varying NOT NULL,
    "indexedAt" character varying NOT NULL,
    "sortAt" character varying GENERATED ALWAYS AS (LEAST("createdAt", "indexedAt")) STORED NOT NULL
);

--
-- Name: like; Type: TABLE
--

CREATE TABLE "like" (
    uri character varying NOT NULL,
    cid character varying NOT NULL,
    creator character varying NOT NULL,
    subject character varying NOT NULL,
    "subjectCid" character varying NOT NULL,
    "createdAt" character varying NOT NULL,
    "indexedAt" character varying NOT NULL,
    "sortAt" character varying GENERATED ALWAYS AS (LEAST("createdAt", "indexedAt")) STORED NOT NULL,
    via character varying,
    "viaCid" character varying,
    "takedownRef" character varying,
    space_uri character varying
);

--
-- Name: list; Type: TABLE
--

CREATE TABLE list (
    uri character varying NOT NULL,
    cid character varying NOT NULL,
    creator character varying NOT NULL,
    name character varying NOT NULL,
    purpose character varying NOT NULL,
    description character varying,
    "descriptionFacets" character varying,
    "avatarCid" character varying,
    "createdAt" character varying NOT NULL,
    "indexedAt" character varying NOT NULL,
    "sortAt" character varying GENERATED ALWAYS AS (LEAST("createdAt", "indexedAt")) STORED NOT NULL
);

--
-- Name: list_block; Type: TABLE
--

CREATE TABLE list_block (
    uri character varying NOT NULL,
    cid character varying NOT NULL,
    creator character varying NOT NULL,
    "subjectUri" character varying NOT NULL,
    "createdAt" character varying NOT NULL,
    "indexedAt" character varying NOT NULL,
    "sortAt" character varying GENERATED ALWAYS AS (LEAST("createdAt", "indexedAt")) STORED NOT NULL
);

--
-- Name: list_item; Type: TABLE
--

CREATE TABLE list_item (
    uri character varying NOT NULL,
    cid character varying NOT NULL,
    creator character varying NOT NULL,
    "subjectDid" character varying NOT NULL,
    "listUri" character varying NOT NULL,
    "createdAt" character varying NOT NULL,
    "indexedAt" character varying NOT NULL,
    "sortAt" character varying GENERATED ALWAYS AS (LEAST("createdAt", "indexedAt")) STORED NOT NULL
);

--
-- Name: list_mute; Type: TABLE
--

CREATE TABLE list_mute (
    "listUri" character varying NOT NULL,
    "mutedByDid" character varying NOT NULL,
    "createdAt" character varying NOT NULL
);

--
-- Name: moderation_action; Type: TABLE
--

CREATE TABLE moderation_action (
    id integer NOT NULL,
    action character varying NOT NULL,
    "subjectType" character varying NOT NULL,
    "subjectDid" character varying NOT NULL,
    "subjectUri" character varying,
    "subjectCid" character varying,
    reason text NOT NULL,
    "createdAt" character varying NOT NULL,
    "createdBy" character varying NOT NULL,
    "reversedAt" character varying,
    "reversedBy" character varying,
    "reversedReason" text,
    "createLabelVals" character varying,
    "negateLabelVals" character varying,
    "durationInHours" integer,
    "expiresAt" character varying
);

--
-- Name: moderation_action_id_seq; Type: SEQUENCE
--

CREATE SEQUENCE moderation_action_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

--
-- Name: moderation_action_id_seq; Type: SEQUENCE OWNED BY
--

ALTER SEQUENCE moderation_action_id_seq OWNED BY moderation_action.id;

--
-- Name: moderation_action_subject_blob; Type: TABLE
--

CREATE TABLE moderation_action_subject_blob (
    "actionId" integer NOT NULL,
    cid character varying NOT NULL
);

--
-- Name: moderation_event; Type: TABLE
--

CREATE TABLE moderation_event (
    id integer NOT NULL,
    action character varying NOT NULL,
    "subjectType" character varying NOT NULL,
    "subjectDid" character varying NOT NULL,
    "subjectUri" character varying,
    "subjectCid" character varying,
    comment text,
    meta jsonb,
    "createdAt" character varying NOT NULL,
    "createdBy" character varying NOT NULL,
    "reversedAt" character varying,
    "reversedBy" character varying,
    "durationInHours" integer,
    "expiresAt" character varying,
    "reversedReason" text,
    "createLabelVals" character varying,
    "negateLabelVals" character varying,
    "legacyRefId" integer
);

--
-- Name: moderation_event_id_seq; Type: SEQUENCE
--

CREATE SEQUENCE moderation_event_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

--
-- Name: moderation_event_id_seq; Type: SEQUENCE OWNED BY
--

ALTER SEQUENCE moderation_event_id_seq OWNED BY moderation_event.id;

--
-- Name: moderation_report; Type: TABLE
--

CREATE TABLE moderation_report (
    id integer NOT NULL,
    "subjectType" character varying NOT NULL,
    "subjectDid" character varying NOT NULL,
    "subjectUri" character varying,
    "subjectCid" character varying,
    "reasonType" character varying NOT NULL,
    reason text,
    "reportedByDid" character varying NOT NULL,
    "createdAt" character varying NOT NULL
);

--
-- Name: moderation_report_id_seq; Type: SEQUENCE
--

CREATE SEQUENCE moderation_report_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

--
-- Name: moderation_report_id_seq; Type: SEQUENCE OWNED BY
--

ALTER SEQUENCE moderation_report_id_seq OWNED BY moderation_report.id;

--
-- Name: moderation_report_resolution; Type: TABLE
--

CREATE TABLE moderation_report_resolution (
    "reportId" integer NOT NULL,
    "actionId" integer NOT NULL,
    "createdBy" character varying NOT NULL,
    "createdAt" character varying NOT NULL
);

--
-- Name: moderation_subject_status; Type: TABLE
--

CREATE TABLE moderation_subject_status (
    id integer NOT NULL,
    did character varying NOT NULL,
    "recordPath" character varying DEFAULT ''::character varying NOT NULL,
    "blobCids" jsonb,
    "recordCid" character varying,
    "reviewState" character varying NOT NULL,
    comment character varying,
    "muteUntil" character varying,
    "lastReviewedAt" character varying,
    "lastReviewedBy" character varying,
    "lastReportedAt" character varying,
    takendown boolean DEFAULT false NOT NULL,
    "suspendUntil" character varying,
    "createdAt" character varying NOT NULL,
    "updatedAt" character varying NOT NULL
);

--
-- Name: moderation_subject_status_id_seq; Type: SEQUENCE
--

CREATE SEQUENCE moderation_subject_status_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

--
-- Name: moderation_subject_status_id_seq; Type: SEQUENCE OWNED BY
--

ALTER SEQUENCE moderation_subject_status_id_seq OWNED BY moderation_subject_status.id;

--
-- Name: mute; Type: TABLE
--

CREATE TABLE mute (
    "subjectDid" character varying NOT NULL,
    "mutedByDid" character varying NOT NULL,
    "createdAt" character varying NOT NULL,
    "onlyReposts" boolean DEFAULT false NOT NULL,
    "onlyQuoteposts" boolean DEFAULT false NOT NULL
);

--
-- Name: notification; Type: TABLE
--

CREATE TABLE notification (
    id bigint NOT NULL,
    did character varying NOT NULL,
    "recordUri" character varying NOT NULL,
    "recordCid" character varying NOT NULL,
    author character varying NOT NULL,
    reason character varying NOT NULL,
    "reasonSubject" character varying,
    "sortAt" character varying NOT NULL
);

--
-- Name: notification_id_seq; Type: SEQUENCE
--

CREATE SEQUENCE notification_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

--
-- Name: notification_id_seq; Type: SEQUENCE OWNED BY
--

ALTER SEQUENCE notification_id_seq OWNED BY notification.id;

--
-- Name: notification_push_outbox; Type: TABLE
--

CREATE TABLE notification_push_outbox (
    id character varying NOT NULL,
    "notificationId" bigint,
    did character varying NOT NULL,
    "recordUri" character varying NOT NULL,
    "recordCid" character varying NOT NULL,
    author character varying NOT NULL,
    reason character varying NOT NULL,
    "reasonSubject" character varying,
    "sortAt" character varying NOT NULL,
    "courierNotificationId" character varying NOT NULL,
    status character varying DEFAULT 'pending'::character varying NOT NULL,
    attempts integer DEFAULT 0 NOT NULL,
    "nextAttemptAt" timestamp with time zone DEFAULT now() NOT NULL,
    "expiresAt" timestamp with time zone NOT NULL,
    "lastError" character varying,
    "createdAt" timestamp with time zone DEFAULT now() NOT NULL,
    "updatedAt" timestamp with time zone DEFAULT now() NOT NULL
);

--
-- Name: notification_push_token; Type: TABLE
--

CREATE TABLE notification_push_token (
    did character varying NOT NULL,
    platform character varying NOT NULL,
    token character varying NOT NULL,
    "appId" character varying NOT NULL
);

--
-- Name: op_thread_reply; Type: TABLE
--

CREATE TABLE op_thread_reply (
    "rootUri" character varying NOT NULL,
    "parentUri" character varying NOT NULL,
    uri character varying NOT NULL,
    "deletedAt" character varying
);

--
-- Name: peer_mod_label; Type: TABLE
--

CREATE TABLE peer_mod_label (
    id bigint NOT NULL,
    "subjectUri" character varying NOT NULL,
    "subjectCid" character varying NOT NULL,
    val character varying NOT NULL,
    "peerModDid" character varying NOT NULL,
    "ozoneEventId" character varying DEFAULT ''::character varying NOT NULL,
    "createdAt" character varying NOT NULL,
    "negatedAt" character varying,
    "negatedBy" character varying,
    "negationOzoneEventId" character varying
);

--
-- Name: peer_mod_label_id_seq; Type: SEQUENCE
--

CREATE SEQUENCE peer_mod_label_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;

--
-- Name: peer_mod_label_id_seq; Type: SEQUENCE OWNED BY
--

ALTER SEQUENCE peer_mod_label_id_seq OWNED BY peer_mod_label.id;

--
-- Name: post_embed_external; Type: TABLE
--

CREATE TABLE post_embed_external (
    "postUri" character varying NOT NULL,
    uri character varying NOT NULL,
    title character varying NOT NULL,
    description character varying NOT NULL,
    "thumbCid" character varying
);

--
-- Name: post_embed_gallery_image; Type: TABLE
--

CREATE TABLE post_embed_gallery_image (
    "postUri" character varying NOT NULL,
    "position" character varying NOT NULL,
    "imageCid" character varying NOT NULL,
    alt character varying NOT NULL
);

--
-- Name: post_embed_image; Type: TABLE
--

CREATE TABLE post_embed_image (
    "postUri" character varying NOT NULL,
    "position" character varying NOT NULL,
    "imageCid" character varying NOT NULL,
    alt character varying NOT NULL
);

--
-- Name: post_embed_record; Type: TABLE
--

CREATE TABLE post_embed_record (
    "postUri" character varying NOT NULL,
    "embedUri" character varying NOT NULL,
    "embedCid" character varying NOT NULL
);

--
-- Name: post_embed_video; Type: TABLE
--

CREATE TABLE post_embed_video (
    "postUri" character varying NOT NULL,
    "videoCid" character varying NOT NULL,
    alt character varying
);

--
-- Name: post_gate; Type: TABLE
--

CREATE TABLE post_gate (
    uri character varying NOT NULL,
    cid character varying NOT NULL,
    creator character varying NOT NULL,
    "postUri" character varying NOT NULL,
    "createdAt" character varying NOT NULL,
    "indexedAt" character varying NOT NULL
);

--
-- Name: private_data; Type: TABLE
--

CREATE TABLE private_data (
    "actorDid" character varying NOT NULL,
    namespace character varying NOT NULL,
    key character varying NOT NULL,
    payload text NOT NULL,
    "indexedAt" character varying NOT NULL,
    "updatedAt" character varying NOT NULL
);

--
-- Name: profile; Type: TABLE
--

CREATE TABLE profile (
    uri character varying NOT NULL,
    cid character varying NOT NULL,
    creator character varying NOT NULL,
    "displayName" character varying,
    description character varying,
    "avatarCid" character varying,
    "bannerCid" character varying,
    "indexedAt" character varying NOT NULL,
    "joinedViaStarterPackUri" character varying,
    "createdAt" character varying NOT NULL,
    "pinnedPost" character varying,
    "pinnedPostCid" character varying
);

--
-- Name: profile_agg; Type: TABLE
--

CREATE TABLE profile_agg (
    did character varying NOT NULL,
    "followersCount" bigint DEFAULT 0 NOT NULL,
    "followsCount" bigint DEFAULT 0 NOT NULL,
    "postsCount" bigint DEFAULT 0 NOT NULL
);

--
-- Name: quote; Type: TABLE
--

CREATE TABLE quote (
    uri character varying NOT NULL,
    cid character varying NOT NULL,
    subject character varying NOT NULL,
    "subjectCid" character varying NOT NULL,
    "createdAt" character varying NOT NULL,
    "indexedAt" character varying NOT NULL,
    "sortAt" character varying GENERATED ALWAYS AS (LEAST("createdAt", "indexedAt")) STORED NOT NULL
);

--
-- Name: record; Type: TABLE
--

CREATE TABLE record (
    uri character varying NOT NULL,
    cid character varying NOT NULL,
    did character varying NOT NULL,
    "json" text NOT NULL,
    "indexedAt" character varying NOT NULL,
    "takedownRef" character varying,
    tags jsonb,
    rev character varying
);

--
-- Name: repo_sync; Type: TABLE
--

CREATE TABLE repo_sync (
    did character varying NOT NULL,
    rev character varying NOT NULL,
    data_cid character varying NOT NULL,
    host character varying,
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);

--
-- Name: repost; Type: TABLE
--

CREATE TABLE repost (
    uri character varying NOT NULL,
    cid character varying NOT NULL,
    creator character varying NOT NULL,
    subject character varying NOT NULL,
    "subjectCid" character varying NOT NULL,
    "createdAt" character varying NOT NULL,
    "indexedAt" character varying NOT NULL,
    "sortAt" character varying GENERATED ALWAYS AS (LEAST("createdAt", "indexedAt")) STORED NOT NULL,
    via character varying,
    "viaCid" character varying,
    "takedownRef" character varying
);

--
-- Name: starter_pack; Type: TABLE
--

CREATE TABLE starter_pack (
    uri character varying NOT NULL,
    cid character varying NOT NULL,
    creator character varying NOT NULL,
    "createdAt" character varying NOT NULL,
    "indexedAt" character varying NOT NULL,
    "sortAt" character varying GENERATED ALWAYS AS (LEAST("createdAt", "indexedAt")) STORED NOT NULL,
    name character varying
);

--
-- Name: sub_state; Type: TABLE
--

CREATE TABLE sub_state (
    service text NOT NULL,
    cursor bigint NOT NULL
);

--
-- Name: subscription; Type: TABLE
--

CREATE TABLE subscription (
    service character varying NOT NULL,
    method character varying NOT NULL,
    state character varying NOT NULL
);

--
-- Name: suggested_feed; Type: TABLE
--

CREATE TABLE suggested_feed (
    uri character varying NOT NULL,
    "order" integer NOT NULL
);

--
-- Name: suggested_follow; Type: TABLE
--

CREATE TABLE suggested_follow (
    did character varying NOT NULL,
    "order" integer NOT NULL
);

--
-- Name: tagged_suggestion; Type: TABLE
--

CREATE TABLE tagged_suggestion (
    tag character varying NOT NULL,
    subject character varying NOT NULL,
    "subjectType" character varying NOT NULL
);

--
-- Name: thread_gate; Type: TABLE
--

CREATE TABLE thread_gate (
    uri character varying NOT NULL,
    cid character varying NOT NULL,
    creator character varying NOT NULL,
    "postUri" character varying NOT NULL,
    "createdAt" character varying NOT NULL,
    "indexedAt" character varying NOT NULL
);

--
-- Name: thread_mute; Type: TABLE
--

CREATE TABLE thread_mute (
    "rootUri" character varying NOT NULL,
    "mutedByDid" character varying NOT NULL,
    "createdAt" character varying NOT NULL
);

--
-- Name: verification; Type: TABLE
--

CREATE TABLE verification (
    uri character varying NOT NULL,
    cid character varying NOT NULL,
    rkey character varying NOT NULL,
    creator character varying NOT NULL,
    subject character varying NOT NULL,
    handle character varying NOT NULL,
    "displayName" character varying NOT NULL,
    "createdAt" character varying NOT NULL,
    "indexedAt" character varying NOT NULL,
    "sortedAt" character varying GENERATED ALWAYS AS (LEAST("createdAt", "indexedAt")) STORED NOT NULL
);

--
-- Name: actor_badge id; Type: DEFAULT
--

ALTER TABLE ONLY actor_badge ALTER COLUMN id SET DEFAULT nextval('actor_badge_id_seq'::regclass);

--
-- Name: moderation_action id; Type: DEFAULT
--

ALTER TABLE ONLY moderation_action ALTER COLUMN id SET DEFAULT nextval('moderation_action_id_seq'::regclass);

--
-- Name: moderation_event id; Type: DEFAULT
--

ALTER TABLE ONLY moderation_event ALTER COLUMN id SET DEFAULT nextval('moderation_event_id_seq'::regclass);

--
-- Name: moderation_report id; Type: DEFAULT
--

ALTER TABLE ONLY moderation_report ALTER COLUMN id SET DEFAULT nextval('moderation_report_id_seq'::regclass);

--
-- Name: moderation_subject_status id; Type: DEFAULT
--

ALTER TABLE ONLY moderation_subject_status ALTER COLUMN id SET DEFAULT nextval('moderation_subject_status_id_seq'::regclass);

--
-- Name: notification id; Type: DEFAULT
--

ALTER TABLE ONLY notification ALTER COLUMN id SET DEFAULT nextval('notification_id_seq'::regclass);

--
-- Name: peer_mod_label id; Type: DEFAULT
--

ALTER TABLE ONLY peer_mod_label ALTER COLUMN id SET DEFAULT nextval('peer_mod_label_id_seq'::regclass);

--
-- Name: activity_subscription activity_subscription_pkey; Type: CONSTRAINT
--

ALTER TABLE ONLY activity_subscription
    ADD CONSTRAINT activity_subscription_pkey PRIMARY KEY (creator, key);

--
-- Name: activity_subscription activity_subscription_unique_creator_subject_did; Type: CONSTRAINT
--

ALTER TABLE ONLY activity_subscription
    ADD CONSTRAINT activity_subscription_unique_creator_subject_did UNIQUE (creator, "subjectDid");

--
-- Name: actor_badge actor_badge_pkey; Type: CONSTRAINT
--

ALTER TABLE ONLY actor_badge
    ADD CONSTRAINT actor_badge_pkey PRIMARY KEY (id);

--
-- Name: actor_block actor_block_pkey; Type: CONSTRAINT
--

ALTER TABLE ONLY actor_block
    ADD CONSTRAINT actor_block_pkey PRIMARY KEY (uri);

--
-- Name: actor_block actor_block_unique_subject; Type: CONSTRAINT
--

ALTER TABLE ONLY actor_block
    ADD CONSTRAINT actor_block_unique_subject UNIQUE (creator, "subjectDid");

--
-- Name: actor actor_handle_key; Type: CONSTRAINT
--

ALTER TABLE ONLY actor
    ADD CONSTRAINT actor_handle_key UNIQUE (handle);

--
-- Name: actor actor_pkey; Type: CONSTRAINT
--

ALTER TABLE ONLY actor
    ADD CONSTRAINT actor_pkey PRIMARY KEY (did);

--
-- Name: actor_state actor_state_pkey; Type: CONSTRAINT
--

ALTER TABLE ONLY actor_state
    ADD CONSTRAINT actor_state_pkey PRIMARY KEY (did);

--
-- Name: actor_sync actor_sync_pkey; Type: CONSTRAINT
--

ALTER TABLE ONLY actor_sync
    ADD CONSTRAINT actor_sync_pkey PRIMARY KEY (did);

--
-- Name: blob_takedown blob_takedown_pkey; Type: CONSTRAINT
--

ALTER TABLE ONLY blob_takedown
    ADD CONSTRAINT blob_takedown_pkey PRIMARY KEY (did, cid);

--
-- Name: bookmark bookmark_pkey; Type: CONSTRAINT
--

ALTER TABLE ONLY bookmark
    ADD CONSTRAINT bookmark_pkey PRIMARY KEY (creator, key);

--
-- Name: bookmark bookmark_unique_uri_creator; Type: CONSTRAINT
--

ALTER TABLE ONLY bookmark
    ADD CONSTRAINT bookmark_unique_uri_creator UNIQUE ("subjectUri", creator);

--
-- Name: community_post community_post_pkey; Type: CONSTRAINT
--

ALTER TABLE ONLY community_post
    ADD CONSTRAINT community_post_pkey PRIMARY KEY (uri);

--
-- Name: did_cache did_cache_pkey; Type: CONSTRAINT
--

ALTER TABLE ONLY did_cache
    ADD CONSTRAINT did_cache_pkey PRIMARY KEY (did);

--
-- Name: draft draft_pkey; Type: CONSTRAINT
--

ALTER TABLE ONLY draft
    ADD CONSTRAINT draft_pkey PRIMARY KEY (creator, key);

--
-- Name: duplicate_record duplicate_record_pkey; Type: CONSTRAINT
--

ALTER TABLE ONLY duplicate_record
    ADD CONSTRAINT duplicate_record_pkey PRIMARY KEY (uri);

--
-- Name: feed_generator feed_generator_pkey; Type: CONSTRAINT
--

ALTER TABLE ONLY feed_generator
    ADD CONSTRAINT feed_generator_pkey PRIMARY KEY (uri);

--
-- Name: feed_item feed_item_pkey; Type: CONSTRAINT
--

ALTER TABLE ONLY feed_item
    ADD CONSTRAINT feed_item_pkey PRIMARY KEY (uri);

--
-- Name: follow follow_pkey; Type: CONSTRAINT
--

ALTER TABLE ONLY follow
    ADD CONSTRAINT follow_pkey PRIMARY KEY (uri);

--
-- Name: follow follow_unique_subject; Type: CONSTRAINT
--

ALTER TABLE ONLY follow
    ADD CONSTRAINT follow_unique_subject UNIQUE (creator, "subjectDid");

--
-- Name: kysely_migration_lock kysely_migration_lock_pkey; Type: CONSTRAINT
--

ALTER TABLE ONLY kysely_migration_lock
    ADD CONSTRAINT kysely_migration_lock_pkey PRIMARY KEY (id);

--
-- Name: kysely_migration kysely_migration_pkey; Type: CONSTRAINT
--

ALTER TABLE ONLY kysely_migration
    ADD CONSTRAINT kysely_migration_pkey PRIMARY KEY (name);

--
-- Name: label label_pkey; Type: CONSTRAINT
--

ALTER TABLE ONLY label
    ADD CONSTRAINT label_pkey PRIMARY KEY (src, uri, cid, val);

--
-- Name: labeler labeler_pkey; Type: CONSTRAINT
--

ALTER TABLE ONLY labeler
    ADD CONSTRAINT labeler_pkey PRIMARY KEY (uri);

--
-- Name: like like_pkey; Type: CONSTRAINT
--

ALTER TABLE ONLY "like"
    ADD CONSTRAINT like_pkey PRIMARY KEY (uri);

--
-- Name: like like_unique_subject; Type: CONSTRAINT
--

ALTER TABLE ONLY "like"
    ADD CONSTRAINT like_unique_subject UNIQUE (subject, creator);

--
-- Name: list_block list_block_pkey; Type: CONSTRAINT
--

ALTER TABLE ONLY list_block
    ADD CONSTRAINT list_block_pkey PRIMARY KEY (uri);

--
-- Name: list_block list_block_unique_subject; Type: CONSTRAINT
--

ALTER TABLE ONLY list_block
    ADD CONSTRAINT list_block_unique_subject UNIQUE (creator, "subjectUri");

--
-- Name: list_item list_item_pkey; Type: CONSTRAINT
--

ALTER TABLE ONLY list_item
    ADD CONSTRAINT list_item_pkey PRIMARY KEY (uri);

--
-- Name: list_item list_item_unique_subject_in_list; Type: CONSTRAINT
--

ALTER TABLE ONLY list_item
    ADD CONSTRAINT list_item_unique_subject_in_list UNIQUE ("listUri", "subjectDid");

--
-- Name: list_mute list_mute_pkey; Type: CONSTRAINT
--

ALTER TABLE ONLY list_mute
    ADD CONSTRAINT list_mute_pkey PRIMARY KEY ("mutedByDid", "listUri");

--
-- Name: list list_pkey; Type: CONSTRAINT
--

ALTER TABLE ONLY list
    ADD CONSTRAINT list_pkey PRIMARY KEY (uri);

--
-- Name: moderation_action moderation_action_pkey; Type: CONSTRAINT
--

ALTER TABLE ONLY moderation_action
    ADD CONSTRAINT moderation_action_pkey PRIMARY KEY (id);

--
-- Name: moderation_action_subject_blob moderation_action_subject_blob_pkey; Type: CONSTRAINT
--

ALTER TABLE ONLY moderation_action_subject_blob
    ADD CONSTRAINT moderation_action_subject_blob_pkey PRIMARY KEY ("actionId", cid);

--
-- Name: moderation_event moderation_event_pkey; Type: CONSTRAINT
--

ALTER TABLE ONLY moderation_event
    ADD CONSTRAINT moderation_event_pkey PRIMARY KEY (id);

--
-- Name: moderation_report moderation_report_pkey; Type: CONSTRAINT
--

ALTER TABLE ONLY moderation_report
    ADD CONSTRAINT moderation_report_pkey PRIMARY KEY (id);

--
-- Name: moderation_report_resolution moderation_report_resolution_pkey; Type: CONSTRAINT
--

ALTER TABLE ONLY moderation_report_resolution
    ADD CONSTRAINT moderation_report_resolution_pkey PRIMARY KEY ("reportId", "actionId");

--
-- Name: moderation_subject_status moderation_status_unique_idx; Type: CONSTRAINT
--

ALTER TABLE ONLY moderation_subject_status
    ADD CONSTRAINT moderation_status_unique_idx UNIQUE (did, "recordPath");

--
-- Name: moderation_subject_status moderation_subject_status_pkey; Type: CONSTRAINT
--

ALTER TABLE ONLY moderation_subject_status
    ADD CONSTRAINT moderation_subject_status_pkey PRIMARY KEY (id);

--
-- Name: mute mute_pkey; Type: CONSTRAINT
--

ALTER TABLE ONLY mute
    ADD CONSTRAINT mute_pkey PRIMARY KEY ("mutedByDid", "subjectDid");

--
-- Name: notification notification_pkey; Type: CONSTRAINT
--

ALTER TABLE ONLY notification
    ADD CONSTRAINT notification_pkey PRIMARY KEY (id);

--
-- Name: notification_push_outbox notification_push_outbox_pkey; Type: CONSTRAINT
--

ALTER TABLE ONLY notification_push_outbox
    ADD CONSTRAINT notification_push_outbox_pkey PRIMARY KEY (id);

--
-- Name: notification_push_token notification_push_token_pkey; Type: CONSTRAINT
--

ALTER TABLE ONLY notification_push_token
    ADD CONSTRAINT notification_push_token_pkey PRIMARY KEY (did, token);

--
-- Name: op_thread_reply op_thread_reply_pkey; Type: CONSTRAINT
--

ALTER TABLE ONLY op_thread_reply
    ADD CONSTRAINT op_thread_reply_pkey PRIMARY KEY ("rootUri", uri);

--
-- Name: peer_mod_label peer_mod_label_pkey; Type: CONSTRAINT
--

ALTER TABLE ONLY peer_mod_label
    ADD CONSTRAINT peer_mod_label_pkey PRIMARY KEY (id);

--
-- Name: post_agg post_agg_pkey; Type: CONSTRAINT
--

ALTER TABLE ONLY post_agg
    ADD CONSTRAINT post_agg_pkey PRIMARY KEY (uri);

--
-- Name: post_embed_external post_embed_external_pkey; Type: CONSTRAINT
--

ALTER TABLE ONLY post_embed_external
    ADD CONSTRAINT post_embed_external_pkey PRIMARY KEY ("postUri");

--
-- Name: post_embed_gallery_image post_embed_gallery_image_pkey; Type: CONSTRAINT
--

ALTER TABLE ONLY post_embed_gallery_image
    ADD CONSTRAINT post_embed_gallery_image_pkey PRIMARY KEY ("postUri", "position");

--
-- Name: post_embed_image post_embed_image_pkey; Type: CONSTRAINT
--

ALTER TABLE ONLY post_embed_image
    ADD CONSTRAINT post_embed_image_pkey PRIMARY KEY ("postUri", "position");

--
-- Name: post_embed_record post_embed_record_pkey; Type: CONSTRAINT
--

ALTER TABLE ONLY post_embed_record
    ADD CONSTRAINT post_embed_record_pkey PRIMARY KEY ("postUri", "embedUri");

--
-- Name: post_embed_video post_embed_video_pkey; Type: CONSTRAINT
--

ALTER TABLE ONLY post_embed_video
    ADD CONSTRAINT post_embed_video_pkey PRIMARY KEY ("postUri");

--
-- Name: post_gate post_gate_pkey; Type: CONSTRAINT
--

ALTER TABLE ONLY post_gate
    ADD CONSTRAINT post_gate_pkey PRIMARY KEY (uri);

--
-- Name: post_gate post_gate_postUri_key; Type: CONSTRAINT
--

ALTER TABLE ONLY post_gate
    ADD CONSTRAINT "post_gate_postUri_key" UNIQUE ("postUri");

--
-- Name: post post_pkey; Type: CONSTRAINT
--

ALTER TABLE ONLY post
    ADD CONSTRAINT post_pkey PRIMARY KEY (uri);

--
-- Name: private_data private_data_pkey; Type: CONSTRAINT
--

ALTER TABLE ONLY private_data
    ADD CONSTRAINT private_data_pkey PRIMARY KEY ("actorDid", namespace, key);

--
-- Name: profile_agg profile_agg_pkey; Type: CONSTRAINT
--

ALTER TABLE ONLY profile_agg
    ADD CONSTRAINT profile_agg_pkey PRIMARY KEY (did);

--
-- Name: profile profile_pkey; Type: CONSTRAINT
--

ALTER TABLE ONLY profile
    ADD CONSTRAINT profile_pkey PRIMARY KEY (uri);

--
-- Name: quote quote_pkey; Type: CONSTRAINT
--

ALTER TABLE ONLY quote
    ADD CONSTRAINT quote_pkey PRIMARY KEY (uri);

--
-- Name: record record_pkey; Type: CONSTRAINT
--

ALTER TABLE ONLY record
    ADD CONSTRAINT record_pkey PRIMARY KEY (uri);

--
-- Name: repo_sync repo_sync_pkey; Type: CONSTRAINT
--

ALTER TABLE ONLY repo_sync
    ADD CONSTRAINT repo_sync_pkey PRIMARY KEY (did);

--
-- Name: repost repost_pkey; Type: CONSTRAINT
--

ALTER TABLE ONLY repost
    ADD CONSTRAINT repost_pkey PRIMARY KEY (uri);

--
-- Name: repost repost_unique_subject; Type: CONSTRAINT
--

ALTER TABLE ONLY repost
    ADD CONSTRAINT repost_unique_subject UNIQUE (creator, subject);

--
-- Name: starter_pack starter_pack_pkey; Type: CONSTRAINT
--

ALTER TABLE ONLY starter_pack
    ADD CONSTRAINT starter_pack_pkey PRIMARY KEY (uri);

--
-- Name: sub_state sub_state_pkey; Type: CONSTRAINT
--

ALTER TABLE ONLY sub_state
    ADD CONSTRAINT sub_state_pkey PRIMARY KEY (service);

--
-- Name: subscription subscription_pkey; Type: CONSTRAINT
--

ALTER TABLE ONLY subscription
    ADD CONSTRAINT subscription_pkey PRIMARY KEY (service, method);

--
-- Name: suggested_feed suggested_feed_pkey; Type: CONSTRAINT
--

ALTER TABLE ONLY suggested_feed
    ADD CONSTRAINT suggested_feed_pkey PRIMARY KEY (uri);

--
-- Name: suggested_follow suggested_follow_pkey; Type: CONSTRAINT
--

ALTER TABLE ONLY suggested_follow
    ADD CONSTRAINT suggested_follow_pkey PRIMARY KEY (did);

--
-- Name: tagged_suggestion tagged_suggestion_pkey; Type: CONSTRAINT
--

ALTER TABLE ONLY tagged_suggestion
    ADD CONSTRAINT tagged_suggestion_pkey PRIMARY KEY (tag, subject);

--
-- Name: thread_gate thread_gate_pkey; Type: CONSTRAINT
--

ALTER TABLE ONLY thread_gate
    ADD CONSTRAINT thread_gate_pkey PRIMARY KEY (uri);

--
-- Name: thread_gate thread_gate_postUri_key; Type: CONSTRAINT
--

ALTER TABLE ONLY thread_gate
    ADD CONSTRAINT "thread_gate_postUri_key" UNIQUE ("postUri");

--
-- Name: thread_mute thread_mute_pkey; Type: CONSTRAINT
--

ALTER TABLE ONLY thread_mute
    ADD CONSTRAINT thread_mute_pkey PRIMARY KEY ("rootUri", "mutedByDid");

--
-- Name: verification verification_pkey; Type: CONSTRAINT
--

ALTER TABLE ONLY verification
    ADD CONSTRAINT verification_pkey PRIMARY KEY (uri);

--
-- Name: verification verification_unique_subject_creator; Type: CONSTRAINT
--

ALTER TABLE ONLY verification
    ADD CONSTRAINT verification_unique_subject_creator UNIQUE (subject, creator);

--
-- Name: view_param view_param_pkey; Type: CONSTRAINT
--

ALTER TABLE ONLY view_param
    ADD CONSTRAINT view_param_pkey PRIMARY KEY (name);

--
-- Name: actor_badge_active_unique; Type: INDEX
--

CREATE UNIQUE INDEX actor_badge_active_unique ON actor_badge USING btree (did, badge) WHERE ("revokedAt" IS NULL);

--
-- Name: actor_badge_did_idx; Type: INDEX
--

CREATE INDEX actor_badge_did_idx ON actor_badge USING btree (did) WHERE ("revokedAt" IS NULL);

--
-- Name: actor_block_subjectdid_idx; Type: INDEX
--

CREATE INDEX actor_block_subjectdid_idx ON actor_block USING btree ("subjectDid");

--
-- Name: actor_handle_tgrm_idx; Type: INDEX
--

CREATE INDEX actor_handle_tgrm_idx ON actor USING gist (handle public.gist_trgm_ops);

--
-- Name: algo_whats_hot_view_cursor_idx; Type: INDEX
--

CREATE INDEX algo_whats_hot_view_cursor_idx ON algo_whats_hot_view USING btree (score, cid);

--
-- Name: algo_whats_hot_view_uri_idx; Type: INDEX
--

CREATE UNIQUE INDEX algo_whats_hot_view_uri_idx ON algo_whats_hot_view USING btree (uri);

--
-- Name: community_post_creator_sort_idx; Type: INDEX
--

CREATE INDEX community_post_creator_sort_idx ON community_post USING btree (creator, "sortAt" DESC);

--
-- Name: community_post_reply_root_idx; Type: INDEX
--

CREATE INDEX community_post_reply_root_idx ON community_post USING btree ("replyRoot");

--
-- Name: community_post_sort_idx; Type: INDEX
--

CREATE INDEX community_post_sort_idx ON community_post USING btree ("sortAt" DESC);

--
-- Name: draft_creator_updated_at_key_idx; Type: INDEX
--

CREATE INDEX draft_creator_updated_at_key_idx ON draft USING btree (creator, "updatedAt", key);

--
-- Name: duplicate_record_duplicate_of_idx; Type: INDEX
--

CREATE INDEX duplicate_record_duplicate_of_idx ON duplicate_record USING btree ("duplicateOf");

--
-- Name: feed_generator_creator_index; Type: INDEX
--

CREATE INDEX feed_generator_creator_index ON feed_generator USING btree (creator);

--
-- Name: feed_item_cursor_idx; Type: INDEX
--

CREATE INDEX feed_item_cursor_idx ON feed_item USING btree ("sortAt", cid);

--
-- Name: feed_item_originator_cursor_idx; Type: INDEX
--

CREATE INDEX feed_item_originator_cursor_idx ON feed_item USING btree ("originatorDid", "sortAt", cid);

--
-- Name: feed_item_post_uri_idx; Type: INDEX
--

CREATE INDEX feed_item_post_uri_idx ON feed_item USING btree ("postUri");

--
-- Name: follow_creator_cursor_idx; Type: INDEX
--

CREATE INDEX follow_creator_cursor_idx ON follow USING btree (creator, "sortAt", cid);

--
-- Name: follow_subject_cursor_idx; Type: INDEX
--

CREATE INDEX follow_subject_cursor_idx ON follow USING btree ("subjectDid", "sortAt", cid);

--
-- Name: label_cts_idx; Type: INDEX
--

CREATE INDEX label_cts_idx ON label USING btree (cts);

--
-- Name: label_uri_index; Type: INDEX
--

CREATE INDEX label_uri_index ON label USING btree (uri);

--
-- Name: labeler_order_by_idx; Type: INDEX
--

CREATE INDEX labeler_order_by_idx ON labeler USING btree ("sortAt", cid);

--
-- Name: like_creator_cursor_idx; Type: INDEX
--

CREATE INDEX like_creator_cursor_idx ON "like" USING btree (creator, "sortAt", cid);

--
-- Name: list_creator_idx; Type: INDEX
--

CREATE INDEX list_creator_idx ON list USING btree (creator);

--
-- Name: list_item_creator_idx; Type: INDEX
--

CREATE INDEX list_item_creator_idx ON list_item USING btree (creator);

--
-- Name: list_item_subject_idx; Type: INDEX
--

CREATE INDEX list_item_subject_idx ON list_item USING btree ("subjectDid");

--
-- Name: moderation_action_subject_blob_cid_idx; Type: INDEX
--

CREATE INDEX moderation_action_subject_blob_cid_idx ON moderation_action_subject_blob USING btree (cid);

--
-- Name: moderation_report_resolution_action_id_idx; Type: INDEX
--

CREATE INDEX moderation_report_resolution_action_id_idx ON moderation_report_resolution USING btree ("actionId");

--
-- Name: moderation_subject_status_blob_cids_idx; Type: INDEX
--

CREATE INDEX moderation_subject_status_blob_cids_idx ON moderation_subject_status USING gin ("blobCids");

--
-- Name: notification_author_idx; Type: INDEX
--

CREATE INDEX notification_author_idx ON notification USING btree (author);

--
-- Name: notification_did_recorduri_reason_unique_idx; Type: INDEX
--

CREATE UNIQUE INDEX notification_did_recorduri_reason_unique_idx ON notification USING btree (did, "recordUri", reason);

--
-- Name: notification_did_sortat_idx; Type: INDEX
--

CREATE INDEX notification_did_sortat_idx ON notification USING btree (did, "sortAt");

--
-- Name: notification_push_outbox_due_idx; Type: INDEX
--

CREATE INDEX notification_push_outbox_due_idx ON notification_push_outbox USING btree ("nextAttemptAt") WHERE ((status)::text = ANY ((ARRAY['pending'::character varying, 'retryable'::character varying])::text[]));

--
-- Name: notification_push_outbox_expires_idx; Type: INDEX
--

CREATE INDEX notification_push_outbox_expires_idx ON notification_push_outbox USING btree ("expiresAt") WHERE ((status)::text = ANY ((ARRAY['pending'::character varying, 'retryable'::character varying])::text[]));

--
-- Name: notification_record_idx; Type: INDEX
--

CREATE INDEX notification_record_idx ON notification USING btree ("recordUri");

--
-- Name: peer_mod_label_active_unique; Type: INDEX
--

CREATE UNIQUE INDEX peer_mod_label_active_unique ON peer_mod_label USING btree ("subjectUri", val) WHERE ("negatedAt" IS NULL);

--
-- Name: peer_mod_label_subject_peer_idx; Type: INDEX
--

CREATE INDEX peer_mod_label_subject_peer_idx ON peer_mod_label USING btree ("subjectUri", "peerModDid") WHERE ("negatedAt" IS NULL);

--
-- Name: post_creator_cursor_idx; Type: INDEX
--

CREATE INDEX post_creator_cursor_idx ON post USING btree (creator, "sortAt", cid);

--
-- Name: post_order_by_idx; Type: INDEX
--

CREATE INDEX post_order_by_idx ON post USING btree ("sortAt", cid);

--
-- Name: post_replyparent_idx; Type: INDEX
--

CREATE INDEX post_replyparent_idx ON post USING btree ("replyParent") INCLUDE (uri);

--
-- Name: profile_creator_idx; Type: INDEX
--

CREATE INDEX profile_creator_idx ON profile USING btree (creator);

--
-- Name: profile_display_name_tgrm_idx; Type: INDEX
--

CREATE INDEX profile_display_name_tgrm_idx ON profile USING gist ("displayName" public.gist_trgm_ops);

--
-- Name: profile_starter_pack_joined_idx; Type: INDEX
--

CREATE INDEX profile_starter_pack_joined_idx ON profile USING btree ("joinedViaStarterPackUri", "createdAt");

--
-- Name: quote_subject_cursor_idx; Type: INDEX
--

CREATE INDEX quote_subject_cursor_idx ON quote USING btree (subject, "sortAt", cid);

--
-- Name: record_did_idx; Type: INDEX
--

CREATE INDEX record_did_idx ON record USING btree (did);

--
-- Name: repost_order_by_idx; Type: INDEX
--

CREATE INDEX repost_order_by_idx ON repost USING btree ("sortAt", cid);

--
-- Name: repost_subject_idx; Type: INDEX
--

CREATE INDEX repost_subject_idx ON repost USING btree (subject);

--
-- Name: starter_pack_creator_order_by_idx; Type: INDEX
--

CREATE INDEX starter_pack_creator_order_by_idx ON starter_pack USING btree (creator, "sortAt", cid);

--
-- Name: starter_pack_name_tgrm_idx; Type: INDEX
--

CREATE INDEX starter_pack_name_tgrm_idx ON starter_pack USING gist (name public.gist_trgm_ops);

--
-- Name: notification notification_push_insert_notify; Type: TRIGGER
--

CREATE TRIGGER notification_push_insert_notify AFTER INSERT ON notification FOR EACH ROW EXECUTE FUNCTION notify_notification_push_insert();

--
-- Name: moderation_action_subject_blob moderation_action_subject_blob_actionId_fkey; Type: FK CONSTRAINT
--

ALTER TABLE ONLY moderation_action_subject_blob
    ADD CONSTRAINT "moderation_action_subject_blob_actionId_fkey" FOREIGN KEY ("actionId") REFERENCES moderation_action(id);

--
-- Name: moderation_report_resolution moderation_report_resolution_actionId_fkey; Type: FK CONSTRAINT
--

ALTER TABLE ONLY moderation_report_resolution
    ADD CONSTRAINT "moderation_report_resolution_actionId_fkey" FOREIGN KEY ("actionId") REFERENCES moderation_action(id);

--
-- Name: moderation_report_resolution moderation_report_resolution_reportId_fkey; Type: FK CONSTRAINT
--

ALTER TABLE ONLY moderation_report_resolution
    ADD CONSTRAINT "moderation_report_resolution_reportId_fkey" FOREIGN KEY ("reportId") REFERENCES moderation_report(id);
