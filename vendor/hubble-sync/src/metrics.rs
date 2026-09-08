//! all the metrics
//!
//! apps should call [`describe_metrics`] once at startup

use metrics::{describe_counter, describe_gauge, describe_histogram};

// ///// registry

/// did->repo actor lookups. `result` = `hit` | `miss`
pub(crate) const REPO_REGISTRY_LOADS_TOTAL: &str = "hubble_sync_repo_actor_loads_total";

/// successful repo actor evictions
pub(crate) const REPO_REGISTRY_EVICTIONS_TOTAL: &str = "hubble_sync_repo_actor_evictions_total";

/// repo actor lookup fail due to no registry space freeable to revive
pub(crate) const REPO_REGISTRY_EVICTION_STUCK_TOTAL: &str =
    "hubble_sync_repo_actor_eviction_stuck_total";

/// repo actors currently alive
pub(crate) const REPO_REGISTRY_ACTIVE: &str = "hubble_sync_repo_actor_active";

/// repo actors actively doing something. `task` = `Bootstrap:*` or `Task::name()` outcome
pub(crate) const REPO_ACTOR_WORKING: &str = "hubble_sync_repo_actor_working";

/// histogram of actor-alive-durations
pub(crate) const REPO_ACTOR_ALIVE_SECONDS: &str = "hubble_sync_repo_actor_alive_seconds";

/// actor revives. `wake` = `resolved` | `pending` | `refreshing` | `failed`
pub(crate) const REPO_ACTOR_REVIVES_TOTAL: &str = "hubble_sync_repo_actor_revives_total";

/// total repos. `status` =  `synchronized` | `desynchronized` | `deactivated` | `gone`
pub(crate) const REPOS_TOTAL: &str = "hubble_sync_repos_total";

/// total repos by (effective) account status.
/// `status` = `active` | `deactivated` | `suspended` | `takendown` | `deleted` | `other`
pub(crate) const REPOS_BY_ACCOUNT_STATUS: &str = "hubble_sync_repos_by_account_status";

/// desynchronized repos broken out by desync reason (drill-down of the
/// `desynchronized` bucket of [`REPOS_TOTAL`]). `reason` = `first_seen`
/// | `unresolvable_identity` | `firehose_sync` | `firehose_fail`
/// | `firehose_account_desync` | `firehose_account_throttled` | `future_rev`
/// | `throttled` | `sync11_lax` | `app_requested`
pub(crate) const REPOS_DESYNC_BY_REASON: &str = "hubble_sync_repos_desync_by_reason";

/// new-did resolves. `outcome` = `get_up` | `snooze` | `retry` | `storage_error`
pub(crate) const INITIAL_RESOLVE_OUTCOMES_TOTAL: &str =
    "hubble_sync_initial_resolve_outcomes_total";

/// identity refreshes. `outcome` = `refreshed` | `snoozed` | `failed` | `storage_error`,
/// `trigger` = `wake` | `sig_retry` | `firehose_identity` | `resync_repo_missing` | `requested`
pub(crate) const IDENTITY_REFRESH_OUTCOMES_TOTAL: &str =
    "hubble_sync_identity_refresh_outcomes_total";

/// actual requests made to resolve identity. `did_method` = `plc` | `web`; `status` = <code> | `transport
pub(crate) const IDENTITY_RESOLVE_REQUESTS_TOTAL: &str =
    "hubble_sync_identity_resolve_requests_total";

/// time to actually resolve identities by did method. `did_method` = `plc` | `web`
pub(crate) const IDENTITY_RESOLVE_DURATION_SECONDS: &str =
    "hubble_sync_identity_resolve_duration_seconds";

/// identity resolution outcomes by did method and result. `did_method` = `plc` | `web`; `result` = `ok` | `not_found` | `rate_limited` | `request_error` | `unresolveable`
pub(crate) const IDENTITY_RESOLVE_OUTCOMES_TOTAL: &str =
    "hubble_sync_identity_resolve_outcomes_total";

// ///// host registry / host state

/// hostname->Host lookups. `result` = `hit` | `miss`
pub(crate) const HOST_REGISTRY_LOADS_TOTAL: &str = "hubble_sync_host_registry_loads_total";

/// upstream rate-limits or transient failures. `status` =  `429` | `502` | `503` | `504`
pub(crate) const HOST_BACKOFFS_TOTAL: &str = "hubble_sync_host_backoffs_total";

/// sync1.1 compliance ratchet upgrades (Lax → Strict). monotonic per host
/// instance, so this is roughly "how many distinct hosts we've ratcheted
/// since startup"
pub(crate) const HOST_SYNC_COMPLIANCE_UPGRADES_TOTAL: &str =
    "hubble_sync_host_sync_compliance_upgrades_total";

// ///// resync queue + scheduler

/// resync queue rows written (incl. on FirstSeen during repo creation
/// and on actor-driven re-enqueue)
pub(crate) const RESYNC_QUEUE_ENQUEUED_TOTAL: &str = "hubble_sync_resync_queue_enqueued_total";

/// resync queue rows deleted (on actor-side resync completion)
pub(crate) const RESYNC_QUEUE_DEQUEUED_TOTAL: &str = "hubble_sync_resync_queue_dequeued_total";

/// resync not dispatched because resync concurrency exhausted
pub(crate) const RESYNC_CONCURRENCY_SATURATED_TOTAL: &str =
    "hubble_sync_resync_concurrency_saturated_total";

/// in-memory scheduler dispatches (caller called next() and got a due
/// resync back). reflects scheduler throughput, not queue depth.
pub(crate) const RESYNC_SCHEDULER_DISPATCHED_TOTAL: &str =
    "hubble_sync_resync_scheduler_dispatched_total";

/// hosts currently scheduled in the round-robin
pub(crate) const RESYNC_SCHEDULER_SCHEDULED: &str = "hubble_sync_resync_scheduler_scheduled";

/// length of the round-robin ring itself. diverges from `_scheduled` (the
/// `hosts` map size) if a host is in the map but absent from the ring — i.e.
/// scheduled-but-never-dispatched.
pub(crate) const RESYNC_SCHEDULER_ROUND_ROBIN_LEN: &str =
    "hubble_sync_resync_scheduler_round_robin_len";

/// scheduled hosts broken down by why `next()` would (or wouldn't) dispatch
/// them, to diagnose dispatch imbalance.
/// `state` = `deliverable` | `not_due` | `at_max` | `backoff` | `not_in_ring`
pub(crate) const RESYNC_SCHEDULER_HOSTS_BY_STATE: &str =
    "hubble_sync_resync_scheduler_hosts_by_state";

/// dispatches blocked at the per-second rate limiter (governor backpressure)
pub(crate) const RESYNC_DISPATCH_THROTTLED_TOTAL: &str =
    "hubble_sync_resync_dispatch_throttled_total";

/// dispatches skipped because the host's request_concurrency permits were
/// exhausted (soft check — host is saturated, slot returned to round-robin)
pub(crate) const RESYNC_HOST_BACKPRESSURED_TOTAL: &str =
    "hubble_sync_resync_host_backpressured_total";

/// re-bootstrap passes run
pub(crate) const RESYNC_REBOOTSTRAP_PASSES_TOTAL: &str =
    "hubble_sync_resync_rebootstrap_passes_total";

/// entries added by re-bootstrap (newly-discovered hosts, dropped refills)
pub(crate) const RESYNC_REBOOTSTRAP_ADDED_TOTAL: &str =
    "hubble_sync_resync_rebootstrap_added_total";

/// dispatches that failed to reach the actor mailbox (registry saturation
/// or evictions stuck)
pub(crate) const RESYNC_DISPATCH_FAILED_TOTAL: &str = "hubble_sync_resync_dispatch_failed_total";

// ///// event processing outcomes

/// #commit processing outcomes.
/// `outcome` = `applied` | `desync` | `drop_sig` | `drop_inactive` | `drop_desynced` | `drop_stale`
pub(crate) const COMMIT_OUTCOMES_TOTAL: &str = "hubble_sync_commit_outcomes_total";

/// scheduled-resync processing outcomes.
/// `outcome` = `synced` | `reschedule` | `status` | `repo_missing` | `deleted` | `cancelled` | `spurious`
pub(crate) const RESYNC_OUTCOMES_TOTAL: &str = "hubble_sync_resync_outcomes_total";

/// decoded resync data size, recorded on a completed resync (meaning of "size" depends on ResyncData type)
pub(crate) const RESYNC_SIZE: &str = "hubble_sync_resync_size";

/// resync data record count, recorded on a completed resync
pub(crate) const RESYNC_COUNT: &str = "hubble_sync_resync_count";

/// wall-clock seconds for a completed resync (fetch + load + apply)
pub(crate) const RESYNC_DURATION_SECONDS: &str = "hubble_sync_resync_duration_seconds";

/// resync time by each phase of fetch, `phase` = `limit_wait` (getRepo: queued
/// at host limits, all redirect steps if any) | `request` (send + response
/// headers, net of limit_wait) | `drain` (streaming the response body)
pub(crate) const RESYNC_PHASE_SECONDS: &str = "hubble_sync_resync_phase_seconds";

/// seconds spent waiting to acquire a big-repo permit during resync.
/// `path` = `preemptive` (predicted big up front) | `reactive` (small load overflowed)
pub(crate) const BIG_REPO_WAIT_SECONDS: &str = "hubble_sync_big_repo_wait_seconds";

/// big-repo resync permits currently held
pub(crate) const BIG_REPO_PERMITS_IN_USE: &str = "hubble_sync_big_repo_permits_in_use";

/// configured total big permits
pub(crate) const BIG_REPO_PERMITS_LIMIT: &str = "hubble_sync_big_repo_permits_limit";

/// #sync processing outcomes.
/// `outcome` = `desync` | `drop_sig` | `drop_desynced` | `drop_stale`
pub(crate) const SYNC_OUTCOMES_TOTAL: &str = "hubble_sync_sync_outcomes_total";

/// #account processing outcomes.
/// `outcome` = `changed` | `noop`
pub(crate) const ACCOUNT_OUTCOMES_TOTAL: &str = "hubble_sync_account_outcomes_total";

// ///// pending identity resolution

pub(crate) const PENDING_IDENTITY_ALREADY_RESOLVED_TOTAL: &str =
    "hubble_sync_pending_identity_already_resolved_total";

pub(crate) const PENDING_IDENTITY_SCHEDULER_DISPATCHED_TOTAL: &str =
    "hubble_sync_pending_identity_scheduler_dispatched_total";

pub(crate) const PENDING_IDENTITY_SCHEDULER_SCHEDULED: &str =
    "hubble_sync_pending_identity_scheduler_scheduled";

/// resolve-retry keys written (counts entry + time-ordered queue)
pub(crate) const PENDING_IDENTITY_QUEUE_ENQUEUED_TOTAL: &str =
    "hubble_sync_pending_identity_queue_enqueued_total";

/// resolve-retry keys rows deleted (counts entry + time-ordered queue)
pub(crate) const PENDING_IDENTITY_QUEUE_DEQUEUED_TOTAL: &str =
    "hubble_sync_pending_identity_queue_dequeued_total";

/// repos submitted for backfill by out-of-band (off-firehose) discovery
/// strategies (ie `RepoDiscovery::request_sync`)
pub(crate) const BACKFILL_REQUESTED_TOTAL: &str = "hubble_sync_backfill_requested_total";

/// repos submitted out-of-band (off-firehose) that actually got enqueued
pub(crate) const BACKFILL_ENQUEUED_TOTAL: &str = "hubble_sync_backfill_enqueued_total";

// ///// firehose

/// seconds for sync1.1 prevalidation including op-inversion
pub(crate) const COMMIT_PREVALIDATE_SECONDS: &str = "hubble_sync_commit_prevalidate_seconds";

/// usable subscribeRepos messages. `kind` = `commit` | `sync` | `identity` | `account` | `info` | `unknown`
pub(crate) const FIREHOSE_MESSAGES_TOTAL: &str = "hubble_sync_firehose_messages_total";

/// unusuable subscribeRepos messages (`i64` seq was negative)
pub(crate) const FIREHOSE_INVALID_SEQ_TOTAL: &str = "hubble_sync_firehose_invalid_seq_total";

/// unusable subscribeRepos message (prevalidation failed), `stage` = 'pre' | 'sig' | 'rev' | 'chain'
pub(crate) const FIREHOSE_VALIDATE_FAIL_TOTAL: &str = "hubble_sync_firehose_validate_fail_total";

/// non-fatal websocket-stream-level decode errors
pub(crate) const FIREHOSE_DECODE_ERRORS_TOTAL: &str = "hubble_sync_firehose_decode_errors_total";

/// repo-level backpressure on dispatch (bad sign)
pub(crate) const FIREHOSE_REPO_BACKPRESSURE_RETRIES_TOTAL: &str =
    "hubble_sync_firehose_repo_backpressure_retries_total";

/// repo-level retry on backpressure give-ups
pub(crate) const FIREHOSE_REPO_BACKPRESSURE_EXHAUSTED_TOTAL: &str =
    "hubble_sync_firehose_repo_backpressure_exhausted_total";

/// events that don't get acked (and won't get replayed on app restart)
pub(crate) const FIREHOSE_STALL_EVICTIONS_TOTAL: &str =
    "hubble_sync_firehose_stall_evictions_total";

/// current number of un-acked events. increases eventually lead to firehose backpressure
pub(crate) const FIREHOSE_OUTSTANDING: &str = "hubble_sync_firehose_outstanding";

/// connection attempts. `result` = `success` | `failed`
pub(crate) const FIREHOSE_CONNECTS_TOTAL: &str = "hubble_sync_firehose_connects_total";

/// reconnect cycles by trigger. `reason` = `stream_end` | `idle_timeout` | `transport_error` | `closed`.
pub(crate) const FIREHOSE_RECONNECTS_TOTAL: &str = "hubble_sync_firehose_reconnects_total";

/// last persisted firehose cursor seq.
pub(crate) const FIREHOSE_CURSOR_SEQ: &str = "hubble_sync_firehose_cursor_seq";

/// replay lag: seconds between an upstream event's `time` and when we received
/// it. high while catching up (replay/backfill), ~0 once live, negative if the
/// upstream sends timestamps ahead of our clock.
pub(crate) const FIREHOSE_UPSTREAM_LAG_SECONDS: &str = "hubble_sync_firehose_upstream_lag_seconds";

// ///// outbound requests

/// total requests sent
pub(crate) const REQUESTS_SENT_TOTAL: &str = "hubble_sync_requests_sent_total";

/// total responses received
pub(crate) const RESPONSES_RECEIVED_TOTAL: &str = "hubble_sync_responses_received_total";

/// seconds a request spent queued at its host's limits (concurrency permit +
/// rate limiter) before actually sending. recorded per redirect step.
pub(crate) const HOST_LIMIT_WAIT_SECONDS: &str = "hubble_sync_host_limit_wait_seconds";

/// register metrics descriptions
///
/// call once early at app startup
pub fn describe_metrics() {
    // registry
    describe_counter!(
        REPO_REGISTRY_LOADS_TOTAL,
        "did->actor lookups, result=hit|miss"
    );
    describe_counter!(
        REPO_REGISTRY_EVICTIONS_TOTAL,
        "actor evictions that succeeded"
    );
    describe_counter!(
        REPO_REGISTRY_EVICTION_STUCK_TOTAL,
        "actor evictions that failed"
    );
    describe_gauge!(REPO_REGISTRY_ACTIVE, "current number of live repo actors");
    describe_histogram!(
        REPO_ACTOR_ALIVE_SECONDS,
        "seconds from revive to exit-guard drop"
    );
    describe_counter!(
        REPO_ACTOR_REVIVES_TOTAL,
        "actor revives, outcome=resolved|pending|refreshing|failed"
    );
    describe_gauge!(
        REPOS_TOTAL,
        "total repos, status=synchronized|desynchronized|nonActive|gone"
    );
    describe_gauge!(
        REPOS_BY_ACCOUNT_STATUS,
        "total repos by effective account status=active|deactivated|suspended|takendown|deleted|other"
    );
    describe_gauge!(
        REPOS_DESYNC_BY_REASON,
        "desynchronized repos by desync reason=first_seen|unresolvable_identity|firehose_sync|firehose_fail|firehose_account_desync|..."
    );
    describe_counter!(
        INITIAL_RESOLVE_OUTCOMES_TOTAL,
        "initial resolve, outcome=get_up|snooze|retry|storage_error"
    );
    describe_counter!(
        IDENTITY_REFRESH_OUTCOMES_TOTAL,
        "identity refreshes, outcome=refreshed|snoozed|failed|storage_error, trigger=wake|sig_retry|firehose_identity|resync_repo_missing|requested"
    );
    describe_counter!(
        IDENTITY_RESOLVE_REQUESTS_TOTAL,
        "actual requests made to resolve identity, did_method=plc|web, status=<code>|transport"
    );
    describe_histogram!(
        IDENTITY_RESOLVE_DURATION_SECONDS,
        "time to actually resolve identities by did method, did_method=plc|web"
    );
    describe_counter!(
        IDENTITY_RESOLVE_OUTCOMES_TOTAL,
        "identity resolution outcomes by did method and result, did_method=plc|web; result=ok|not_found|rate_limited|request_error|unresolveable"
    );

    // host registry / host state
    describe_counter!(
        HOST_REGISTRY_LOADS_TOTAL,
        "hostname->Host lookups, result=hit|miss"
    );
    describe_counter!(
        HOST_BACKOFFS_TOTAL,
        "upstream rate-limits or transient failures, status=429|502|503|504",
    );
    describe_counter!(
        HOST_SYNC_COMPLIANCE_UPGRADES_TOTAL,
        "sync1.1 compliance upgrades (Lax->Strict)"
    );

    // resync queue + scheduler
    describe_counter!(RESYNC_QUEUE_ENQUEUED_TOTAL, "resync queue rows written");
    describe_counter!(
        RESYNC_QUEUE_DEQUEUED_TOTAL,
        "resync queue rows deleted (on resync completion)"
    );
    describe_counter!(
        RESYNC_CONCURRENCY_SATURATED_TOTAL,
        "resync not dispatched because resync concurrency exhausted"
    );
    describe_counter!(
        RESYNC_SCHEDULER_DISPATCHED_TOTAL,
        "in-memory resync scheduler dispatches"
    );
    describe_gauge!(
        RESYNC_SCHEDULER_SCHEDULED,
        "hosts currently scheduled in the resync round-robin"
    );
    describe_gauge!(
        RESYNC_SCHEDULER_ROUND_ROBIN_LEN,
        "length of the resync round-robin ring (diverges from _scheduled if hosts leak out of the ring)"
    );
    describe_gauge!(
        RESYNC_SCHEDULER_HOSTS_BY_STATE,
        "scheduled hosts by dispatchability, state=deliverable|not_due|at_max|backoff|not_in_ring"
    );
    describe_counter!(
        RESYNC_DISPATCH_THROTTLED_TOTAL,
        "resync dispatches blocked at the per-second rate limiter"
    );
    describe_counter!(
        RESYNC_HOST_BACKPRESSURED_TOTAL,
        "resync dispatches skipped due to no available host permits"
    );
    describe_counter!(
        RESYNC_DISPATCH_FAILED_TOTAL,
        "resync dispatches that failed to reach the actor mailbox"
    );
    describe_counter!(
        RESYNC_REBOOTSTRAP_PASSES_TOTAL,
        "resync queue re-bootstraps to find added hosts"
    );
    describe_counter!(
        RESYNC_REBOOTSTRAP_ADDED_TOTAL,
        "hosts added by re-bootstrap (were absent/dropped-previously from in-mem scheduler)"
    );

    // pending identity resolution retries
    describe_counter!(
        PENDING_IDENTITY_ALREADY_RESOLVED_TOTAL,
        "pending identity resolutions avoided (already resolved)"
    );
    describe_counter!(
        PENDING_IDENTITY_SCHEDULER_DISPATCHED_TOTAL,
        "pending identity resolution dispatches"
    );
    describe_gauge!(
        PENDING_IDENTITY_SCHEDULER_SCHEDULED,
        "identities currently scheduled in the retry queue"
    );
    describe_counter!(
        PENDING_IDENTITY_QUEUE_ENQUEUED_TOTAL,
        "resolve-retry queue rows written"
    );
    describe_counter!(
        PENDING_IDENTITY_QUEUE_DEQUEUED_TOTAL,
        "resolve-retry queue rows deleted"
    );
    describe_counter!(
        BACKFILL_REQUESTED_TOTAL,
        "repos submitted for backfill (not including firehose-discovered repos)"
    );
    describe_counter!(
        BACKFILL_ENQUEUED_TOTAL,
        "repos actually enqueue for backfill (not including firehose-discovered repos)"
    );

    // firehose
    describe_histogram!(
        COMMIT_PREVALIDATE_SECONDS,
        "seconds for sync1.1 setup and op-inversion (prevData proof) per commit",
    );
    describe_counter!(
        FIREHOSE_MESSAGES_TOTAL,
        "usable subscribeRepos messages, kind=commit|sync|identity|account|info|unknown"
    );
    describe_counter!(
        FIREHOSE_INVALID_SEQ_TOTAL,
        "unusable subscribeRepos messages dropped due to bad sequence number"
    );
    describe_counter!(
        FIREHOSE_VALIDATE_FAIL_TOTAL,
        "subscribeRepos messages dropped in prevalidation, stage=pre|sig|rev|chain"
    );
    describe_counter!(
        FIREHOSE_DECODE_ERRORS_TOTAL,
        "non-fatal websocket-stream-level decode errors"
    );
    describe_counter!(
        FIREHOSE_REPO_BACKPRESSURE_RETRIES_TOTAL,
        "individual backpressure retry attempts during dispatch"
    );
    describe_counter!(
        FIREHOSE_REPO_BACKPRESSURE_EXHAUSTED_TOTAL,
        "dispatches that exhausted the per-DID backpressure retry budget"
    );
    describe_counter!(
        FIREHOSE_STALL_EVICTIONS_TOTAL,
        "un-acked seqs left behind by the cursor-persist watermark"
    );
    describe_gauge!(
        FIREHOSE_OUTSTANDING,
        "current total dispatched but unacked events"
    );
    describe_counter!(
        FIREHOSE_CONNECTS_TOTAL,
        "websocket connection attempts, result=success|failed"
    );
    describe_counter!(
        FIREHOSE_RECONNECTS_TOTAL,
        "reconnect cycles, reason=stream_end|idle_timeout|transport_error|closed"
    );
    describe_gauge!(FIREHOSE_CURSOR_SEQ, "last persisted firehose cursor seq");
    describe_gauge!(
        FIREHOSE_UPSTREAM_LAG_SECONDS,
        "seconds between the last received upstream event's `time` and now (replay lag; ~0 live; negative if upstream time is ahead of ours)"
    );

    // event processing outcomes
    describe_counter!(
        COMMIT_OUTCOMES_TOTAL,
        "#commit outcomes, outcome=applied|desync|drop_sig|drop_inactive|drop_desynced|drop_stale"
    );
    describe_counter!(
        RESYNC_OUTCOMES_TOTAL,
        "resync outcomes, outcome=synced|reschedule|status|repo_missing|deleted|cancelled|spurious"
    );
    describe_histogram!(
        RESYNC_SIZE,
        "decoded resync data size, whose meaning depends on ResyncData type"
    );
    describe_histogram!(RESYNC_COUNT, "resync data record count");
    describe_histogram!(
        RESYNC_DURATION_SECONDS,
        "seconds for a completed resync (fetch+load+apply)"
    );
    describe_histogram!(
        RESYNC_PHASE_SECONDS,
        "seconds for a resync fetch phase, phase=limit_wait|request|drain"
    );
    describe_histogram!(
        BIG_REPO_WAIT_SECONDS,
        "seconds waiting for a big-repo permit, path=preemptive|reactive"
    );
    describe_gauge!(
        BIG_REPO_PERMITS_IN_USE,
        "big-repo resync permits currently held"
    );
    describe_gauge!(BIG_REPO_PERMITS_LIMIT, "configured total big permits",);
    describe_counter!(
        SYNC_OUTCOMES_TOTAL,
        "#sync outcomes, outcome=desync|drop_sig|drop_desynced|drop_stale"
    );
    describe_counter!(
        ACCOUNT_OUTCOMES_TOTAL,
        "#account outcomes, outcome=changed|noop"
    );

    // outbound requests
    describe_counter!(REQUESTS_SENT_TOTAL, "total outbound requests sent");
    describe_counter!(
        RESPONSES_RECEIVED_TOTAL,
        "total responses received from outbound request, by status code"
    );
    describe_histogram!(
        HOST_LIMIT_WAIT_SECONDS,
        "seconds (pre redirect step) a request queued at its host's limits"
    );
}

// ///// recommended histogram buckets

/// for [`IDENTITY_RESOLVE_DURATION_SECONDS`]
pub const IDENTITY_RESOLVE_DURATION_SECONDS_BUCKETS: &[f64] = &[
    0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
];

/// for [`COMMIT_PREVALIDATE_SECONDS`]
pub const COMMIT_PREVALIDATE_SECONDS_BUCKETS: &[f64] =
    &[0.0001, 0.0005, 0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0];

/// for [`RESYNC_SIZE`] (by default this is total size of car *blocks*)
pub const RESYNC_SIZE_BUCKETS: &[f64] = &[
    65_536.0,         // 64 KiB
    262_144.0,        // 256 KiB
    1_048_576.0,      // 1 MiB
    2_097_152.0,      // 2 MiB   — small-tier (MEM_LIMIT_SMALL_MB)
    8_388_608.0,      // 8 MiB
    33_554_432.0,     // 32 MiB
    157_286_400.0,    // 150 MiB — big-tier (MEM_LIMIT_LARGE_MB)
    536_870_912.0,    // 512 MiB
    2_147_483_648.0,  // 2 GiB
    10_737_418_240.0, // 10 GiB  — spill max (SPILL_MAX_STORED_MB)
];

/// for [`RESYNC_COUNT`] (by default this is records emitted in apply_resync)
pub const RESYNC_COUNT_BUCKETS: &[f64] = &[
    1.,
    10.,
    100.,
    1_000.,
    10_000.,
    100_000.,
    1_000_000.,
    10_000_000.,
];

/// for [`RESYNC_DURATION_SECONDS`].
///
/// STREAM_CAR_TIMEOUT is 300s
pub const RESYNC_DURATION_SECONDS_BUCKETS: &[f64] = &[
    0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0, 120.0, 300.0,
];

/// for [`RESYNC_PHASE_SECONDS`]
pub const RESYNC_PHASE_SECONDS_BUCKETS: &[f64] = &[
    0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0, 120.0, 300.0, 600.0,
];

/// for [`HOST_LIMIT_WAIT_SECONDS`]
pub const HOST_LIMIT_WAIT_SECONDS_BUCKETS: &[f64] = &[
    0.001, 0.005, 0.025, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0, 120.0, 300.0, 600.0,
];

/// for [`REPO_ACTOR_ALIVE_SECONDS`]
pub const REPO_ACTOR_ALIVE_SECONDS_BUCKETS: &[f64] = &[
    1.0, 5.0, 30.0, 60.0, 300.0, 1_800.0, 3_600.0, 21_600.0, 86_400.0,
];

/// for [`BIG_REPO_WAIT_SECONDS`]
pub const BIG_REPO_WAIT_SECONDS_BUCKETS: &[f64] =
    &[0.001, 0.01, 0.1, 0.5, 1.0, 5.0, 10.0, 30.0, 60.0, 300.0];

/// recommended `(metric name, buckets)` pairs for every histogram hubble-sync
/// emits, for the app to feed into its exporter at startup (alongside
/// [`describe_metrics`]). e.g. with metrics-exporter-prometheus:
///
/// ```ignore
/// let mut builder = PrometheusBuilder::new();
/// for (name, buckets) in hubble_sync::histogram_buckets() {
///     builder = builder.set_buckets_for_metric(Matcher::Full((*name).into()), buckets)?;
/// }
/// ```
pub fn histogram_buckets() -> &'static [(&'static str, &'static [f64])] {
    &[
        (
            IDENTITY_RESOLVE_DURATION_SECONDS,
            IDENTITY_RESOLVE_DURATION_SECONDS_BUCKETS,
        ),
        (
            COMMIT_PREVALIDATE_SECONDS,
            COMMIT_PREVALIDATE_SECONDS_BUCKETS,
        ),
        (RESYNC_SIZE, RESYNC_SIZE_BUCKETS),
        (RESYNC_COUNT, RESYNC_COUNT_BUCKETS),
        (RESYNC_DURATION_SECONDS, RESYNC_DURATION_SECONDS_BUCKETS),
        (RESYNC_PHASE_SECONDS, RESYNC_PHASE_SECONDS_BUCKETS),
        (HOST_LIMIT_WAIT_SECONDS, HOST_LIMIT_WAIT_SECONDS_BUCKETS),
        (REPO_ACTOR_ALIVE_SECONDS, REPO_ACTOR_ALIVE_SECONDS_BUCKETS),
        (BIG_REPO_WAIT_SECONDS, BIG_REPO_WAIT_SECONDS_BUCKETS),
    ]
}
