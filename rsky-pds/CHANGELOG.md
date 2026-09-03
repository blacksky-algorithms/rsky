# Changelog

All notable changes to `rsky-pds` are documented here.

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
