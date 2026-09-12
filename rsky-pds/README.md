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
| `PDS_DEV_MODE` | Enable development mode |
| `PDS_ADMIN_PASS` | Admin password for admin endpoints |
| `PDS_CONTACT_EMAIL_ADDRESS` | Contact email in server metadata |
| `PDS_PRIVACY_POLICY_URL`, `PDS_TERMS_OF_SERVICE_URL` | Policy links |
| `PDS_ACCEPTING_REPO_IMPORTS` | Allow `importRepo` (default true) |
| `PDS_MAX_REPO_IMPORT_SIZE` | Largest `importRepo` body in bytes (default 100 MiB) |
| `PDS_READ_ONLY` | Serve reads only (see below) |
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
| `PDS_OAUTH_SIGNUP_URL` | Signup URL shown on the authorization page |
| `PDS_OAUTH_TRUSTED_CLIENTS` | Comma-separated client IDs shown by name on the consent page |

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

## Upgrading to 1.0

Every account now signs its repo with its own key. New accounts get one automatically; existing
accounts keep using the previously shared key until an operator runs `rotate-keys`, which publishes
a PLC operation per account. See [MIGRATING-1.0.md](MIGRATING-1.0.md).

## License

rsky is released under the [Apache License 2.0](../LICENSE).
