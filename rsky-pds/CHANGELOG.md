# Changelog

All notable changes to `rsky-pds` are documented here.

## [1.2.0]

### Changed — reference-compatible schema ledgers and account schema

rsky-pds now records the migrations it shares with the reference TypeScript
PDS in Kysely's `kysely_migration` ledger under the reference migration names,
and its own additions in a separate `migrations` ledger. `account.sqlite`
follows the reference schema migration for migration (`passwordScrypt`
columns, no rsky-only columns). A database created by either implementation
can therefore be opened by the other: rsky-pds opens a reference-created
`account.sqlite`, `sequencer.sqlite`, `did_cache.sqlite`, or actor store as-is,
and reads never modify a store's schema. rsky-only actor tables are added on the
first write to a store, not on read.

Databases created by rsky-pds 1.1.x are converted in place on first open: the
misfiled ledger rows move to `kysely_migration`, and `account.sqlite` is
brought to the reference column set (`password` becomes `passwordScrypt`; the
unused `recoveryKey`, `createdAt`, and `inviteNote` columns are dropped).

**This conversion is one-way.** A pre-1.2.0 binary does not know the
`kysely_migration` ledger and will fail to open a converted database. Back up
`account.sqlite` before upgrading if a rollback to 1.1.x must remain possible.


### Added — reference-compatible session tokens

When `PDS_JWT_SECRET` is set, access and refresh tokens are signed with
HMAC-SHA256 over that secret exactly as the reference PDS signs them (same
header, claim order, and lifetimes), so sessions created by either
implementation are valid on the other. Without it, tokens are signed with
ES256K over `PDS_JWT_KEY_K256_PRIVATE_KEY_HEX` as before, and tokens rsky-pds
1.1 issued with the default `JWT` header type stay valid. Tokens now carry
and require the `at+jwt` / `refresh+jwt` types, a taken-down account can log
in with `allowTakendown` and receives the `com.atproto.takendown` scope,
app-password sessions keep their `privileged` flag across refreshes, and
refresh rotation writes the grace period and the successor in one
transaction. Session outputs carry `active` and `status`, `getSession` works
for deactivated accounts, `listAppPasswords` reports `privileged` and accepts
app-password sessions, and rejected credentials answer with the reference
names: `AuthMissing` (401), `AuthenticationRequired` (401), `InvalidToken`
(400), `ExpiredToken` (400), `AccountTakedown` (401).

### Added — reference-compatible OAuth sessions

With `PDS_JWT_SECRET` set the OAuth provider signs access tokens with
HMAC-SHA256 over the shared secret and publishes an empty JWK set, as the
reference PDS does. Access tokens carry the reference claim set, and
verification is stateful: the stored session row is authoritative for the
DPoP key binding, the expiry, and the granted scope (the `token.scope`
column), so a token superseded by a refresh or revoked elsewhere is refused
at once and a session granted a narrower scope than its token claims is
held to the stored grant. DPoP proof replay is tracked in redis under the
reference key scheme when `PDS_REDIS_SCRATCH_ADDRESS` is set, so a proof
consumed by one process is refused by every other. Trusted first-party
clients get the extended session lifetimes. Rejected OAuth credentials
answer with their OAuth error code (`invalid_token`, `use_dpop_nonce`) and
a 401. rsky-oauth 0.4.0 carries the signing-key and replay-store changes.

### Added — account lifecycle parity

`deleteAccount` is the public method the reference PDS exposes (account
password plus the mailed token), and deletion runs in the reference order:
account rows, the deletion event with the earlier history pruned, then the
actor store. Each move is journaled in `PDS_LIFECYCLE_DB` (a tombstone
before the first one, a purge obligation before the actor directory is
unlinked when `PDS_COEXISTENCE` keeps blob storage untouched), no write is
admitted for a DID whose deletion is in progress, and an interrupted
deletion is resumed at the next start. `deactivateAccount` publishes the
account status event, accepts a taken-down account's recovery session, and
`updateHandle` no longer emits the retired `#handle` event. `resetPassword`,
`confirmEmail`, `updateEmail`, `requestAccountDelete`, and `activateAccount`
answer with the reference error names and messages (`InvalidToken`,
`ExpiredToken`, `InvalidEmail`, `AccountNotFound`, `Forbidden` for OAuth
sessions). Session outputs carry `didDoc` when
`PDS_ENABLE_DID_DOC_WITH_SESSION` is set, and a blob that has been uploaded
but not yet referenced by a record is not served.

### Changed — durable writes and publication

A record write is now one SQLite transaction: the repository root is
replaced only if it still equals the root the commit was formatted against,
and the blocks, the record index, the blob bookkeeping, and the write's
publication intent land together or not at all. Blob promotion out of
temporary storage happens before the transaction, and the temporary key is
cleared inside it, so an uploaded blob stays unreadable until the record that
references it is committed. Store write connections, the sequencer, and the
lifecycle journal run with `synchronous=FULL`, so a write acknowledged to a
client survives a power loss.

Publication intents (`publish_intent`, actor migration `005`) are delivered
to the sequencer after the transaction commits and acknowledged in the store
afterwards. The publisher records the sequencer head on the intent before
inserting, so after a crash it recognises a row it already inserted instead
of inserting it again; every write publishes exactly one event. Actors with
undelivered intents are marked in the lifecycle journal (`pending_work`) and
finished at startup.

Object deletions are journaled (`blob_work`) instead of queued in memory.
Under `PDS_COEXISTENCE=true` a dereferenced object is recorded `gc-deferred`
and never deleted; otherwise it is deleted by a worker after the transaction.
Writes are subject to the reference limits of 200 operations and a 2 MB
event per commit, answered with `InvalidRequest`. Repository exports read
from a single snapshot on a dedicated connection, bounded to ten minutes.

### Added — write admission and the maintenance drain

`PDS_WRITE_ALLOWLIST_FILE` names the accounts this process may write while
another implementation shares the data directory. Each entry is `active`,
`draining` (new mutations refused, workers finish), or `maintenance` with a
workflow id; an account the file does not name is refused everywhere. The
file is re-read when it changes and a broken file leaves the previous
allowlist in force. A refused write answers `503 NotAdmitted` with
`Retry-After: 1`. Every write holds a shared `flock` under `PDS_LOCK_DIR`;
`rsky-pds --drain-did <did>` takes it exclusively, finishes the account's
publication and blob work, and reports the same counters that
`GET /xrpc/_drain_status?did=<did>` (admin auth) serves.

### Fixed — responses that differed from the reference PDS

- `com.atproto.sync.*` reads of a missing, taken-down, or deactivated
  repository answer `RepoNotFound`, `RepoTakendown`, or `RepoDeactivated`
  (HTTP 400) instead of an internal error.
- `com.atproto.sync.listRepos` and `getRepoStatus` report a taken-down
  repository as `takendown`, the value the lexicon defines.
- `com.atproto.repo.describeRepo` answers the same `RepoNotFound`,
  `RepoTakendown`, and `RepoDeactivated` errors as the reference PDS, and
  `com.atproto.repo.listRecords` answers `InvalidRequest` for a repository it
  does not serve, instead of internal errors.
- `com.atproto.server.describeServer` includes `blobUploadLimit`.
- Outgoing mail is logged and skipped when `PDS_MAILGUN_API_KEY` is unset or
  empty instead of aborting the request.

### Added — reference-PDS compatibility fixture

`tests/fixtures/ts-pds-0.5.27` is a data directory produced by the pinned
reference PDS image (built by `pds-image/verify/build-ts-fixture.sh`), and
`tests/ts_compat_tests.rs` boots rsky-pds over a copy of it and compares
responses with the ones the reference PDS gave. `PDS_COMPAT_DATA_DIR` points
the tests at a freshly built fixture.
## [1.1.0]

### Changed — password hashing switched from Argon2 to scrypt

New and reset passwords are now hashed with scrypt, using the same cost
parameters and stored-hash encoding as the reference TypeScript PDS
(`packages/pds/src/account-manager/helpers/scrypt.ts`), instead of Argon2.
This makes an account row portable between rsky-pds and the TS PDS in either
direction.

**This is backward compatible on the normal upgrade path**: existing
Argon2-hashed rows keep verifying indefinitely, so upgrading in place does not
require a mass password reset and no currently-working login stops working.

**It is a breaking change for anything running rsky-pds < 1.1.0 against data
written by 1.1.0+.** A pre-1.1.0 binary's password verification only
understands Argon2 (PHC `$`-prefixed) hashes. Any account whose password is
newly created, reset, or otherwise re-hashed while running 1.1.0+ gets a
scrypt-format hash (`<hex salt>:<hex derived key>`) that an older binary
cannot parse or verify. Concretely:

- **Rolling back** to < 1.1.0 after running 1.1.0+ will lock out any account
  whose password was set or reset in the interim, until you roll forward
  again (or that user resets their password again under the old binary,
  which restores an Argon2 hash for that account).
- **Mixed-version deployments** (e.g. a rolling/canary upgrade where old and
  new binaries serve traffic against the same database concurrently) have the
  same hazard for any account touched by the new binary during the overlap.

App-password salting was also corrected to match upstream's
`sha256(did)[:16]` (hex) scheme; previously rsky-pds used a different salt
derivation for app passwords specifically.

## [1.0.0]

Every account gets its own repo signing key, replacing the single
process-wide keypair. See [MIGRATING-1.0.md](./MIGRATING-1.0.md) for the
operator-facing migration guide, including the new `rotate-keys` binary.
