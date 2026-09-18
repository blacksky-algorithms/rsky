# rsky-pds: Personal Data Server (PDS)

Rust implementation of an atproto PDS.

## Storage

All state lives in SQLite databases and a blobstore. No external database
server is required. With `PDS_DATA_DIRECTORY` set (e.g. `/data`), the layout
is:

```
{data}/account.sqlite                       accounts, sessions, invites, OAuth
{data}/sequencer.sqlite                     firehose event log
{data}/did_cache.sqlite                     DID document cache
{data}/actors/<shard>/<did>/store.sqlite    per-actor repo, records, blobs metadata
{data}/actors/<shard>/<did>/key             per-actor repo signing key
{data}/blobs/                               blob bytes (disk blobstore, via PDS_BLOBSTORE_DISK_LOCATION)
```

`<shard>` is the first two hex chars of the sha256 of the DID. Each database
location can also be overridden individually (see below).

Blobs are stored either on local disk (`PDS_BLOBSTORE_DISK_LOCATION`) or in S3
(`PDS_BLOBSTORE_S3_BUCKET`); setting both is an error.

## Running

```bash
cargo run --release -p rsky-pds
```

Or build the container image from the repo root:

```bash
docker build -f rsky-pds/Dockerfile .
```

Mount a volume at `PDS_DATA_DIRECTORY` to persist data.

## Environment variables

### Core

| Variable | Description |
|---|---|
| `PDS_PORT` | Listen port (default 2583) |
| `PDS_HOSTNAME` | Public hostname (default `localhost`) |
| `PDS_SERVICE_DID` | Service DID (default `did:web:{hostname}`) |
| `PDS_VERSION` | Version string reported by the server |
| `PDS_DEV_MODE` | Enable development mode; also lets outbound requests reach private and plain-http addresses |
| `PDS_ADMIN_PASS` | Admin password for admin endpoints |
| `PDS_CONTACT_EMAIL_ADDRESS` | Contact email in server metadata |
| `PDS_PRIVACY_POLICY_URL`, `PDS_TERMS_OF_SERVICE_URL` | Policy links |
| `PDS_ACCEPTING_REPO_IMPORTS` | Allow `importRepo` (default true) |
| `PDS_MAX_REPO_IMPORT_SIZE` | Largest `importRepo` body in bytes (default 100 MiB) |
| `PDS_READ_ONLY` | Serve reads only (see below) |
| `PDS_SHUTDOWN_GRACE_SECS` | How long in-flight requests may finish after SIGTERM or SIGINT (default 100) |
| `PDS_LOG_FORMAT` | `json` (default; one object per line in the reference PDS's shape), `text`, or `traced` (JSON lines carrying the OpenTelemetry trace and span ids) |
| `OTEL_EXPORTER_OTLP_ENDPOINT`, `OTEL_SERVICE_NAME` | Export request spans over OTLP; nothing is exported when unset |
| `PDS_UPLOAD_SPOOL_DIR` | Where uploads are spooled while hashed and stored (default under the system temp dir) |
| `PDS_MAX_CONCURRENT_EXPORTS` | Repository exports served at once (default 4); further requests wait up to 30 s, then 503 |
| `PDS_MAX_CONCURRENT_BLOB_READS` | Blob downloads served at once (default 32) |
| `PDS_RATE_LIMITS_ENABLED` | Apply the reference PDS's request limits (default false) |
| `PDS_RATE_LIMIT_BYPASS_KEY` | Value of an `x-ratelimit-bypass` header that skips every limit |
| `PDS_RATE_LIMIT_BYPASS_IPS` | Comma-separated addresses that skip every limit |
| `PDS_BLOB_UPLOAD_LIMIT` | Max blob upload size in bytes (default 5MB) |

### Storage

| Variable | Description |
|---|---|
| `PDS_DATA_DIRECTORY` | Base directory for all SQLite databases and actor stores |
| `PDS_ACCOUNT_DB_LOCATION` | Override path to `account.sqlite` |
| `PDS_SEQUENCER_DB_LOCATION` | Override path to `sequencer.sqlite` |
| `PDS_DID_CACHE_DB_LOCATION` | Override path to `did_cache.sqlite` |
| `PDS_ACTOR_STORE_DIRECTORY` | Override actor store directory (default `{data}/actors`) |
| `PDS_ACTOR_STORE_CACHE_SIZE` | Open actor DB cache size (default 100) |
| `PDS_BLOBSTORE_DISK_LOCATION` | Disk blobstore directory |
| `PDS_BLOBSTORE_DISK_TMP_LOCATION` | Temp dir for blob uploads |
| `PDS_BLOBSTORE_S3_BUCKET` | S3 bucket for blobs (mutually exclusive with disk) |
| `PDS_BLOBSTORE_S3_REGION` | Bucket region |
| `PDS_BLOBSTORE_S3_ENDPOINT` | S3-compatible endpoint URL (`AWS_ENDPOINT` is honoured as well) |
| `PDS_BLOBSTORE_S3_FORCE_PATH_STYLE` | Address the bucket in the path rather than the host |
| `PDS_BLOBSTORE_S3_ACCESS_KEY_ID`, `PDS_BLOBSTORE_S3_SECRET_ACCESS_KEY` | Static credentials; both or neither, otherwise the SDK's default chain |

### Keys and secrets

| Variable | Description |
|---|---|
| `PDS_JWT_SECRET` | Shared secret for HMAC-SHA256 access/refresh tokens, interchangeable with the reference PDS; takes precedence over the K-256 key |
| `PDS_JWT_KEY_K256_PRIVATE_KEY_HEX` | K-256 key for signing access/refresh tokens when no `PDS_JWT_SECRET` is set |
| `PDS_REPO_SIGNING_KEY_K256_PRIVATE_KEY_HEX` | K-256 key for signing repo commits |
| `PDS_PLC_ROTATION_KEY_K256_PRIVATE_KEY_HEX` | K-256 PLC rotation key |
| `PDS_DPOP_SECRET` | 32-byte hex secret for OAuth DPoP nonce rotation |
| `PDS_LIFECYCLE_DB` | The account deletion and purge journal (default `<PDS_DATA_DIRECTORY>/rsky/lifecycle.sqlite`), kept outside every actor store |
| `PDS_COEXISTENCE` | `true` when another implementation shares the data directory: nothing is deleted from blob storage, deleted accounts leave a purge obligation for later |
| `PDS_WRITE_ALLOWLIST_FILE` | The write allowlist naming the accounts this process may write (see below); every account is admitted when unset |
| `PDS_LOCK_DIR` | Per-account advisory locks shared with the maintenance drain (default `<PDS_DATA_DIRECTORY>/rsky/locks`) |
| `PDS_BLOB_ATTEMPTS_DB` | The journal of every physical S3 write (default `<PDS_DATA_DIRECTORY>/rsky/blob-attempts.sqlite`); never restore it from a backup |
| `PDS_REPAIR_DB` | The repair and quarantine journal (default `<PDS_DATA_DIRECTORY>/rsky/repair.sqlite`) |
| `PDS_REDIS_SCRATCH_ADDRESS` | `host:port` of a redis used to track DPoP proof replay across processes (with `PDS_REDIS_SCRATCH_PASSWORD`); in-memory when unset |
| `PDS_RECOVERY_DID_KEY` | Optional additional PLC rotation key |

### Identity

| Variable | Description |
|---|---|
| `PDS_DID_PLC_URL` | PLC directory URL (default `https://plc.directory`) |
| `PDS_SERVICE_HANDLE_DOMAINS` | Comma-separated handle suffixes (default `.{hostname}`) |
| `PDS_EXTRA_HANDLE_DOMAINS` | Further handle suffixes served here but not offered at signup |
| `PDS_HANDLE_BACKUP_NAMESERVERS` | Backup nameservers for handle resolution |
| `PDS_ID_RESOLVER_TIMEOUT` | DID/handle resolution timeout (ms) |
| `PDS_DID_CACHE_STALE_TTL`, `PDS_DID_CACHE_MAX_TTL` | DID cache TTLs (ms) |
| `PDS_ENABLE_DID_DOC_WITH_SESSION` | Include DID doc in session responses |

### Upstream services

| Variable | Description |
|---|---|
| `PDS_BSKY_APP_VIEW_URL`, `PDS_BSKY_APP_VIEW_DID` | AppView to proxy reads to |
| `PDS_BSKY_APP_VIEW_CDN_URL_PATTERN` | CDN URL pattern for image links |
| `PDS_MOD_SERVICE_URL`, `PDS_MOD_SERVICE_DID` | Moderation service |
| `PDS_REPORT_SERVICE_URL`, `PDS_REPORT_SERVICE_DID` | Report service |
| `PDS_ENTRYWAY_URL`, `PDS_ENTRYWAY_DID` | Entryway, if used |
| `PDS_CRAWLERS` | Comma-separated relay hosts to request crawls from |

### Invites, mail, subscription

| Variable | Description |
|---|---|
| `PDS_INVITE_REQUIRED` | Require invite codes for signup (default true) |
| `PDS_INVITE_INTERVAL`, `PDS_INVITE_EPOCH` | Invite issuance schedule |
| `PDS_EMAIL_FROM_ADDRESS`, `PDS_EMAIL_FROM_NAME` | Transactional mail sender |
| `PDS_MODERATION_EMAIL_FROM_ADDRESS`, `PDS_MODERATION_EMAIL_FROM_NAME` | Moderation mail sender |
| `PDS_MAILGUN_API_KEY`, `PDS_MAILGUN_DOMAIN` | Mailgun credentials |
| `PDS_MAX_SUBSCRIPTION_BUFFER` | Firehose subscriber buffer size |
| `PDS_REPO_BACKFILL_LIMIT_MS` | Backfill window for `subscribeRepos` |

### OAuth

| Variable | Description |
|---|---|
| `PDS_OAUTH_SIGNUP_URL` | External sign-up page linked from the authorization and account pages; when unset, the server's own sign-up is offered instead |
| `PDS_OAUTH_TRUSTED_CLIENTS` | Comma-separated client IDs shown by name and logo on the consent page and allowed to keep their `prompt` |
| `PDS_OAUTH_FIRST_PARTY_CLIENTS` | Comma-separated client IDs operated by this deployment; the identity-takeover warning on consent is not shown for one that is also trusted |
| `PDS_DPOP_SECRET` | 32 hex bytes keying DPoP nonces; random per process when unset |

### Browser pages

The authorization screens and the account manager under `/account` are
server-rendered with the layout, copy and styling of the reference PDS, and
read the same branding variables. Nothing in the pages names a particular
app or organisation unless these are set.

| Variable | Description |
|---|---|
| `PDS_SERVICE_NAME` | The name in the header and footer (default `{hostname} PDS`) |
| `PDS_LOGO_URL` | Logo shown next to the service name |
| `PDS_PRIMARY_COLOR`, `PDS_ERROR_COLOR`, `PDS_WARNING_COLOR`, `PDS_INFO_COLOR`, `PDS_SUCCESS_COLOR` | Brand colours as `#rgb`, `#rrggbb` or `rgb(r, g, b)`; no alpha. A bad value stops startup |
| `PDS_BACKGROUND_LIGHT_URL`, `PDS_BACKGROUND_DARK_URL` | Background images behind the authorization card |
| `PDS_HOME_URL`, `PDS_TERMS_OF_SERVICE_URL`, `PDS_PRIVACY_POLICY_URL`, `PDS_SUPPORT_URL` | Footer links, in that order; unset ones are omitted |
| `PDS_APP_NAME`, `PDS_APP_URL` | The client app the deployment is built around, named in the consent cards ("Access your {app} account"), the deactivate, reactivate and delete copy, and the About page; the wording stays generic when unset |
| `PDS_ORG_NAME` | The operator, linked to `PDS_HOME_URL` under "Learn more" on the About page |
| `PDS_ACCOUNT_UI_ENABLED` | Serve the account manager under `/account` (default true). With it off, only the authorization screens are served |
| `PDS_ACCOUNT_SIGNUP_ENABLED` | Offer this server's own sign-up when `PDS_OAUTH_SIGNUP_URL` is unset and handle domains are configured (default true) |
| `PDS_ACCOUNT_UI_SESSIONS_SINCE` | RFC 3339 instant; device authentications older than it must sign in again before they count for the account pages or for silent consent |

What the pages do, and where they differ from the reference:

- The authorization flow shows the picker of accounts already signed in on
  the device, a plain sign-in form, or the welcome view when sign-up is
  offered; consent lists the requested permissions the way the reference
  does and is skipped when a trusted client already holds a covering grant.
  A sign-in with "Remember this account on this device" unchecked leaves no
  session on the device and carries a short-lived proof through the consent
  form instead. A deactivated account is offered reactivation before consent.
- Accepting or denying sends the browser straight to the client; there is
  no redirect interstitial page.
- A second-factor gate in front of `/oauth/authorize/sign-in` and
  `/account/sign-in` (the Blacksky gatekeeper does this) sends the browser
  back with `otp_hint`, `otp_error`, `auth_error` and `remember`, which the
  pages render. The `csrf` field is `base64url(sha256(device cookie))`.
- The device cookie (`device-id`) is persistent, site-wide, `HttpOnly`,
  `SameSite=Lax` and `Secure` on https. Its secret rotates on sign-in,
  sign-up, sign-out and denial, without a grace period: a tab holding the
  old secret is told the session changed and shown the form again.
- iOS browsers get a cookie probe before the request is bound to a device;
  a browser that never returns it sends the client `invalid_request` with
  `ERR_COOKIES_UNSUPPORTED`. The probe needs one click on "Continue"; the
  reference auto-submits it with JavaScript.
- The account manager offers Home, Account (email, username, password,
  deactivate, reactivate, delete), Devices, Apps and About. Signing in there
  always remembers the account on the device. Deactivation from the pages
  asks for the password and revokes every OAuth session, authorized client
  and app password; the XRPC method keeps them. The password change asks
  for the current password as well as the mailed code. Deletion asks for
  the mailed code and the password, then a final confirmation that carries
  a five-minute signed attestation instead of the password.
- Sign-up has no captcha step; deployments that need one keep
  `PDS_OAUTH_SIGNUP_URL` pointing at a page that provides it.
- `/.well-known/change-password` redirects to `/account/reset-password`.
- English only; permission-set titles in other languages are ignored.
- `cargo run -p rsky-pds --example render_ui -- <dir>` writes every screen
  with sample branding for visual comparison.

## Sharing a data directory

When another PDS implementation writes the same data directory, every
account has exactly one writer at a time. `PDS_WRITE_ALLOWLIST_FILE` names
the accounts this process writes; the file is re-read within seconds of a
change, and a file that fails to parse leaves the previous allowlist in
force:

```toml
version = 1
default = "absent"                 # "active" | "absent"

[entries]
"did:plc:aaaa" = "active"          # writes admitted, workers run
"did:plc:bbbb" = "draining"        # new writes refused (503 NotAdmitted), workers finish
"did:plc:cccc" = { state = "maintenance", workflow_id = "repair-1" }
```

An account the file does not name is refused everywhere, workers included.
Session and token operations are not gated; account and repository
mutations are, and answer `503 NotAdmitted` with `Retry-After: 1`.

`GET /xrpc/community.blacksky.pds.getPublicationFrontier?did=<did>` (admin
auth) reports how far the account's publication history here reaches and
whether it is provably whole, for a downstream index that reconciles
against this server.

Repairs never re-deliver a consumed revision; each lands as a new commit.
`rsky-pds --repair-create <file.json>` records one (`republish` a record at
the same key and value, an `empty-commit` above a revision boundary, or a
`phantom-delete` of a record consumers still hold), `--repair-run <id>`
runs it once the account's allowlist entry is `maintenance` with that id:
it waits for client writes to drain, takes the account's maintenance slot,
records every commit with its step so a crash resumes from the last commit
that landed, and swaps each step against the root the previous step left,
so a change by anyone else ends the repair `client-superseded` rather than
overwritten. `--quarantine-open <seq> --did <did> --kind <kind> [--repair
<id>]...` supersedes the event's intent, invalidates its row, and records the
linked repairs; `--quarantine-local-reconciled <seq>` and
`--quarantine-close <seq> --external verified|accepted [--justification
<text>]` close it only once every linked repair is finished, the local index
was reconciled, and every affected consumer was verified or the gap
explicitly accepted.

`GET /xrpc/community.blacksky.pds.getConvergence?did=<did>` (admin auth)
and `rsky-pds --converge <did>` report whether the account's state here
agrees with what it has published: the store's root, the account database's
root, and the last published commit are one commit; the status and handle
match their last events; and no publication intent, blob work, quarantine,
repair, or deletion is outstanding. A deleted account converges once its
deletion is journaled complete; whether its objects were purged is reported
separately.

`GET /xrpc/_drain_status?did=<did>` (admin auth) reports what an account
still owes this process: in-flight writes, undelivered publication intents,
non-terminal blob work, and an in-progress deletion, with `clientQuiescent`
and `fullyDrained` derived from them. `rsky-pds --drain-did <did>
[--timeout-secs <n>]` waits for the account's in-flight writes, delivers its
intents and runs its blob work under the account's exclusive lock, prints
the same status, and exits 0 only when the account is fully drained.

### Read-only mode

`PDS_READ_ONLY=true` starts a process that only serves reads over a data
directory another process writes. `account.sqlite`, `sequencer.sqlite`,
`did_cache.sqlite`, and every actor store are opened read-only and no
migration runs, so a directory the other implementation created is served
as-is. Every `POST`, `PUT`, `PATCH`, and `DELETE` is answered `503
ReadOnly` before any handler runs; the resolvers cache nothing; deletions
and publication are not resumed. The rsky control journals
(`PDS_LIFECYCLE_DB`, `PDS_REPAIR_DB`, `PDS_BLOB_ATTEMPTS_DB`) stay writable
for the watermarks that reads record.

### Handle routes

Three routes the production edge rewrites to are served beside the
standard ones: `GET /tls-check?domain=` answers `{"success":true}` for the
host itself and for any handle on a served domain (`PDS_SERVICE_HANDLE_DOMAINS`
and `PDS_EXTRA_HANDLE_DOMAINS`), `GET /custom-well-known-atproto-did?handle=`
answers the DID as plain text (falling back to the request host), and
`GET /custom-resolve-handle?handle=` answers `{"did":...}` for local
accounts and resolves foreign handles through the network.

Requests for any well-formed method without a local handler are proxied:
`tools.ozone.*` to `PDS_MOD_SERVICE_URL`, `com.atproto.moderation.createReport`
to `PDS_REPORT_SERVICE_URL`, and everything else to `PDS_BSKY_APP_VIEW_URL`,
unless an `atproto-proxy` header names the service.

### Operating

`GET /metrics` serves Prometheus metrics: requests by route and status
with latency, the same counted by lexicon method, sessions created by
source and outcome, service-auth tokens issued and denied, record writes
by operation, blob uploads and bytes, accounts created, OAuth grants and
bulk session revocations, firehose subscribers, sqlite busy retries, actor
write attempts, control-journal writes by table, rejected credentials by
error, the sequencer head, and per actor the in-flight mutations,
undelivered intents, and nonterminal blob work, with the lifecycle and
repair backlogs. `GET /xrpc/_health` answers 503 while the process drains after a
stop signal. Logs are one JSON object per line in the reference PDS's
shape (`level` 30/40/50, `time`, `pid`, `hostname`, `name`, `msg`, nested
fields), with every request logged as `request completed`.

Repository exports and blob downloads are streamed and bounded by
`PDS_MAX_CONCURRENT_EXPORTS` and `PDS_MAX_CONCURRENT_BLOB_READS`. Uploads
are spooled to disk and refused with `413 PayloadTooLarge` one byte past
`PDS_BLOB_UPLOAD_LIMIT`.

With `PDS_RATE_LIMITS_ENABLED=true` the reference PDS's limits apply: 3000
XRPC requests per address per five minutes (repository exports have their
own 6000), and the per-route limits on session creation, account creation,
uploads, handle updates, password and email flows, and repository writes
(creates 3, updates 2, deletes 1 point against 5000 per hour and 35000 per
day). An exhausted limit answers `429 RateLimitExceeded` with `RateLimit-*`
and `Retry-After` headers. The address is the first `X-Forwarded-For`
entry when present. With `PDS_REDIS_SCRATCH_ADDRESS` the windows live in
that redis under the reference's keys and script (`rl-global-ip:<ip>`,
`rl-repo-write-hour:<did>`, `com.atproto.server.createSession-0:<key>`,
and so on), so a reference PDS sharing the redis charges the same budgets;
a redis that stops answering lets requests through and counts them in
`pds_rate_limit_store_errors_total`. Without it the windows live in this
process's memory.

Account mail (password reset, account deletion, email confirmation and
update, PLC operation) is sent over SMTP from `PDS_EMAIL_SMTP_URL` and
`PDS_EMAIL_FROM_ADDRESS`, as the reference reads them: `smtps://` for
implicit TLS, `smtp://` for STARTTLS, `smtp://...?ignoreTLS=true` for a
plain connection, credentials in the URL. The templates are the branded
ones the reference image sends, rendered here with a text alternative.
Moderation mail from `com.atproto.admin.sendEmail` uses
`PDS_MODERATION_EMAIL_SMTP_URL` and `PDS_MODERATION_EMAIL_ADDRESS` when
set. Without an SMTP URL, Mailgun is used when `PDS_MAILGUN_API_KEY` is
set; without either, messages are logged and the token flows complete.

### After decommission: the blob collector

While `PDS_COEXISTENCE=true` nothing is ever deleted from object storage:
dereferenced objects and deleted accounts leave `gc-deferred` rows and
purge obligations in the journals. Once no other implementation can read
the store, `PDS_BLOB_GC_ENABLED=true` (refused while `PDS_COEXISTENCE` is
set) runs the collector every `PDS_BLOB_GC_INTERVAL_SECS` (3600), and
`rsky-pds --collect <did>` or `--collect-all` runs it once by hand.

Every physical delete is one journaled attempt bound to the exact key it
names, persisted before the request is sent, with the SDK's own retries
disabled. A request the store never confirmed stays `ambiguous`; for a
permanent object the collector then retires the key in the registry at
`PDS_BLOB_GENERATIONS_DB` (`rsky/blob-generations.sqlite`, append-only,
never part of a restore) and the next upload of the same content lands at
a generation key (`blocks/<did>/<cid>.g1`) the old delete never named.
Reads resolve through the registry, and a content whose key has an
unconfirmed delete answers `BlobNotFound` until it does. The collector
refuses to run without the registry, or with a registry that does not know
a retirement the journal records.

A deleted account's namespaces are listed by prefix and every object is
deleted the same way. The obligation reaches `verified-purged` only when
every prefix is empty and every write this process ever made to the
namespace has a confirmed outcome; a namespace another implementation may
have written, or one with no attempt history, is only ever
`observed-empty-legacy-uncertain` and is listed again weekly for the first
quarter after the deletion request and monthly after that, deleting
whatever reappeared. Outcomes are counted in
`pds_blob_collector_outcomes_total{outcome}`.

### Account management without the reference OAuth UI API

The reference PDS serves account mutations to its own authorization UI
under `/@atproto/oauth-provider/~api/*`, authenticated by device-session
cookies. This server has no equivalent of that API: its account manager
under `/account` is server-rendered and posts forms (see "Browser pages"),
so after a cutover those paths answer 410 at the edge, an authorization
page already open in a browser restarts its flow, and existing sessions and
tokens are untouched. Every operation the UI API offered is also served
through XRPC, which is how the Blacksky client performs them:

| UI API endpoint | XRPC equivalent |
|---|---|
| `update-handle` | `com.atproto.identity.updateHandle` |
| `deactivate-account` | `com.atproto.server.deactivateAccount` |
| `reactivate-account` | `com.atproto.server.activateAccount` |
| `delete-account-request`, `delete-account-confirm` | `com.atproto.server.requestAccountDelete`, `com.atproto.server.deleteAccount` |
| `reset-password-request`, `reset-password-confirm` | `com.atproto.server.requestPasswordReset`, `com.atproto.server.resetPassword` |
| `update-email-request`, `update-email-confirm` | `com.atproto.server.requestEmailUpdate`, `com.atproto.server.updateEmail` |
| `verify-email-request`, `verify-email-confirm` | `com.atproto.server.requestEmailConfirmation`, `com.atproto.server.confirmEmail` |
| `revoke-account-session`, `sign-out` | `com.atproto.server.deleteSession` |
| `revoke-oauth-session` | `POST /oauth/revoke` |
| `sign-in`, `sign-up` | `com.atproto.server.createSession`, `com.atproto.server.createAccount` (the gatekeeper's 2FA interception moves to `/oauth/authorize/sign-in`) |

## Testing and coverage

`cargo test -p rsky-pds` runs the unit and integration suites. The
TypeScript-compatibility fixtures under `tests/fixtures/ts-pds-0.5.27/` are
tracked in git (the `data/` directory is exempt from the workspace's
`**/data/` ignore rule) so the compat, read-only, and import-policy suites run
unchanged in CI.

Coverage is enforced by `scripts/coverage-gate.sh <base-ref>` from the
workspace root: every line a change adds or rewrites in `rsky-pds` or
`rsky-oauth` source must be covered. Lines with no code of their own (blank,
comments, attributes, closing brackets, struct and enum headers) are not
gated, since that is where derive-generated code no test can reach is
attributed; function coverage is reported, not gated, because it counts
those generated functions and every per-binary instantiation of a closure.
CI runs the same script against the pushed range, next to its crate-wide 95%
line floor.

## Upgrading to 1.0

Every account now signs its repo with its own key. New accounts get one automatically; existing
accounts keep using the previously shared key until an operator runs `rotate-keys`, which publishes
a PLC operation per account. See [MIGRATING-1.0.md](MIGRATING-1.0.md).

## License

rsky is released under the [Apache License 2.0](../LICENSE).
