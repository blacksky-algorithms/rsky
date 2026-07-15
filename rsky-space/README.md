# rsky-space

Rust primitives for atproto **permissioned data** (spaces) — a parallel protocol
to public atproto broadcast. This crate is the Rust analog of the upstream
`@atproto/space` package: the shared building blocks consumed by a space
**authority/host** and by a **syncer**.

Permissioned data stores a user's records for a shared context ("space") in a
per-user *permissioned repo* on that user's own PDS, summarized by a deniable
LtHash commit, gated by short-lived credentials issued by a space authority.
Apps sync directly from each member's PDS (there is no relay).

## Modules

- `lthash` — homomorphic set-hash commitment over a repo's records (2048-byte
  state, BLAKE3-XOF lanes, `sha256(state)` digest). Order-independent; add/remove
  are single cheap operations.
- `commit` — the domain-separated commit context (`atproto-space-v1`) and
  deniable signature + HKDF/HMAC MAC verification.
- `credential` — delegation tokens, space credentials, and client attestations:
  the JWT envelope, `typ`/claim validation, and signature verification. Signing
  is injected via a closure so the crate stays key-type agnostic.
- `space_id` — `at://{authority}/space/{type}/{skey}/…` addressing and the
  `space` marker that distinguishes permissioned URIs from public ones.
- `types` — `SignedCommit`, `RepoOp`, `RepoRef`.

## Scope of this first pass

**Buildable and tested now** (independent of any PDS): LtHash, the commit
context + verification, credential JWT encode/decode/verify, and space
addressing. These are fully specified by the proposal, so they ship with
conformance-style unit tests.

**Deferred to upstream** (`bluesky-social/atproto` PR #5187): the PDS-side
hosting of permissioned repos and the `com.atproto.space.*` / `getDelegationToken`
methods. A `SpaceRepo` (apply writes, maintain the incremental LtHash, produce/
verify the two-root CAR serialization) and storage adapters build on top of these
modules and reuse `rsky-repo` (CAR, block map, DAG-CBOR); they land once the
serialization details settle upstream.

## Reuse

Reuses `rsky-crypto` (`verify_signature`, did:key), `rsky-common`, and
`rsky-syntax`. Introduces `blake3`/`hkdf`/`hmac`/`subtle` as crate-local deps —
absent from `rsky-crypto` today and candidates to upstream there.
