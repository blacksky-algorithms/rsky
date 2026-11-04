# Migrating to rsky-pds 1.0

1.0 gives every account its own repo signing key. Before it, one process-wide keypair
(`PDS_REPO_SIGNING_KEY_K256_PRIVATE_KEY_HEX`) signed every account's commits, so a single leaked
secret could forge commits — and mint service-auth tokens — for every account on the server.

New accounts get their own key automatically. **Accounts that already exist keep using the shared
key until you rotate them**, which is a deliberate operator step because it publishes a PLC
operation per account.

## What changed

- `createAccount` generates a fresh secp256k1 keypair per account and stamps it into that account's
  PLC operation.
- `getRecommendedDidCredentials`, `submitPlcOperation` and `activateAccount` validate against the
  account's own key instead of the global one.
- Service-auth JWTs are signed with the issuing account's key. This had to change in the same
  release: once an account's DID document names its own key, a token signed with the global key no
  longer verifies.
- New binary `rotate-keys`, built alongside the server.

`PDS_REPO_SIGNING_KEY_K256_PRIVATE_KEY_HEX` is still required. No request path reads it any more,
but `rotate-keys` uses it to tell a rotated account from an unrotated one. Keep it set until every
account has been rotated.

## Upgrading

**1. Deploy 1.0.** No config change. Existing accounts keep working on the shared key; new accounts
get their own from this point on.

**2. See what needs rotating.**

    rotate-keys --dry-run

Reports what would change and touches nothing.

**3. Rotate.** Run against a quiesced PDS — see the warning below.

    rotate-keys                  # every account
    rotate-keys --did did:plc:…  # one account, repeatable

Per account, in order: write the new key file, publish the PLC operation, write an empty commit so
the repo head is signed by the new key, then sequence `#identity` and `#sync`.

**4. Verify.** Re-run `rotate-keys --dry-run`; a clean run reports nothing left to do.

## Before you run it

**Stop the server first.** The per-DID write lock is process-local, so `rotate-keys` cannot
serialise against a live server's in-flight commits. The key write itself is atomic — a concurrent
reader sees whole-old or whole-new, never a truncated file — but a commit signed in the window
between the key write and the PLC operation would be signed with the old key while the DID document
already advertises the new one.

**Back up the actor store.** After rotation each account's key is unique and exists in exactly one
place. Before 1.0 a lost key file could be reconstructed from the environment variable; afterwards
it cannot, and losing one means that account can no longer sign commits.

**It is resumable and idempotent.** An account whose DID document already names its own key is
skipped. An account whose key was written but whose PLC operation did not land — a crash between
steps — is republished rather than re-keyed, so a second key is never minted. Interrupting the run
is safe; re-run it.

**Rotation is one-way.** Downgrading the binary afterwards does not restore the old key, and a
rotated account will not verify against a pre-1.0 server.

**`did:web` accounts are skipped** with a reason. There is no PLC operation to publish for them;
their signing key has to be updated wherever their DID document is served.

## If something fails

The run is sequential and per-account: a failure is counted, logged with its DID, and the run
continues. The binary exits non-zero if any account failed. Re-running retries only what did not
finish.

An account left between steps 1 and 2 — new key on disk, DID document not yet updated — cannot sign
commits that relays will accept until the operation is published. Re-running `rotate-keys` for that
DID resolves it.
