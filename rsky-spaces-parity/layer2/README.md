# Layer 2 — two-process acceptance gate

Layer 1 (`cargo test -p rsky-spaces-parity`) drives both write paths in one
process. Layer 2 runs the two **real binaries** and compares what they actually
serve and store.

```sh
./rsky-spaces-parity/layer2/run.sh
```

Exit code 0 means every scored check was equal. Nothing else is needed: no
Postgres, no Docker, no network. Both servers are SQLite-backed, and the gate
hosts the only support service (a stub DID directory on a loopback port).

## What it does

1. Creates a **detached, build-only** git worktree at the pinned oracle revision
   `7ebd21ae788c550ee8510034d94eb19ede148738` and builds `rsky-pds` there. The
   worktree is refused if it is dirty or at another revision, so the oracle can
   never drift into the code under test.
2. Builds `rsky-space-host` from the working tree.
3. Runs `layer2-gate`, which:
   - starts the stub DID directory, then the oracle PDS, then the space host;
   - creates an account, activates it, opens a session, and creates the space
     `at://<did>/space/community.blacksky.feed/main` on the oracle;
   - copies the oracle's actor-store directory so the space host has the account
     signing keys, and replaces each `store.sqlite` with an empty one created by
     the space host's own schema code;
   - fires the same ten-step record script at each server over XRPC;
   - compares five shared read endpoints;
   - probes the four methods only the PDS routes;
   - stops both servers and points the Layer 1 stored-row comparator
     (`dump_tables` / `compare_tables` / `revs_are_well_formed`) at the two
     `store.sqlite` files.

Everything lands under `target/layer2/run`: `pds.log`, `shim.log`, `report.txt`,
and both store directories. The directory is wiped at the start of every run.

## Credentials

All local, all fixed, all created and destroyed inside the run directory; none
of it is a secret and none of it reaches a real service.

- The oracle PDS takes its own session token on space writes.
- The space host verifies an access token it did not issue, so the gate acts as
  the authorization server: it holds the same HS256 secret the host is
  configured with (`SPACEHOST_OAUTH_HS256_SECRET`) and signs `at+jwt` tokens
  with it, DPoP-bound to a fixed P-256 key. This is the same shape a PDS that is
  its own authorization server issues.
- Read endpoints take a space credential on both sides: minted through
  `getDelegationToken` → `getSpaceCredential` on the PDS, and through
  `/admin/mintCredential` on the space host.

## What is not compared, and why

- `ikm`, `sig`, `mac` on a served commit: derived from fresh random key material
  every serve.
- The paging `cursor`: an oplog row id, numbered per store.
- Revisions: server-minted TIDs. Substituted for `R1, R2, …` in first-appearance
  order, exactly as the Layer 1 comparator does. Record keys are TIDs too and
  compare literally.
- JSON object key order: the two decoders build maps in different orders from
  identical DAG-CBOR bytes. Record identity is the CID, compared unnormalized in
  the same response.
- `getSpace`: recorded as a documented divergence. The space definition is
  configuration on the host and `space_def` rows on the PDS, which the storage
  convergence leaves out of scope by design.

## Running outside a sandbox

On macOS both servers build a `reqwest` client whose proxy discovery calls
SystemConfiguration. A restricted sandbox denies that and the pinned
`hyper-util` panics on the null result. Run the gate with normal process
permissions. `sccache` is also unusable there, so `run.sh` clears
`RUSTC_WRAPPER`.
