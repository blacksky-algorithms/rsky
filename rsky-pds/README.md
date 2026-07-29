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

### Keys and secrets

| Variable | Description |
|---|---|
| `PDS_JWT_KEY_K256_PRIVATE_KEY_HEX` | K-256 key for signing access/refresh tokens |
| `PDS_REPO_SIGNING_KEY_K256_PRIVATE_KEY_HEX` | K-256 key for signing repo commits |
| `PDS_PLC_ROTATION_KEY_K256_PRIVATE_KEY_HEX` | K-256 PLC rotation key |
| `PDS_DPOP_SECRET` | 32-byte hex secret for OAuth DPoP nonce rotation |
| `PDS_RECOVERY_DID_KEY` | Optional additional PLC rotation key |

### Identity

| Variable | Description |
|---|---|
| `PDS_DID_PLC_URL` | PLC directory URL (default `https://plc.directory`) |
| `PDS_SERVICE_HANDLE_DOMAINS` | Comma-separated handle suffixes (default `.{hostname}`) |
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

## License

rsky is released under the [Apache License 2.0](../LICENSE).
