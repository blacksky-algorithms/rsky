# Resume-across-swap gate

Layer 1 and Layer 2 compare the converged space host against the rsky-pds
oracle. This gate asks a different question: when the converged host deploys
over data the legacy host wrote, does the **existing** syncer keep going?

```sh
./rsky-spaces-parity/resume-gate/run.sh
```

Exit code 0 means every scored check passed. No Postgres, no Docker, no
network: three child processes at a time plus two loopback stubs the gate hosts
itself.

## What it does

1. Creates a **detached, build-only** git worktree at `6da61ce`, the
   pre-convergence tip of `feat/space-host-main-port`, and builds
   `rsky-space-host` there. The worktree is refused if it is dirty or at
   another revision.
2. Builds the converged `rsky-space-host`, `convert_store` and `rsky-daemon`
   from the working tree. **The daemon is not modified for this gate** — that
   is the point of it.
3. Runs `resume-gate`, which drives three eras against one space-host database,
   one actor-store directory, one host port and one daemon index:

   **Phase A — legacy era.** The legacy host serves the multi-tenant
   `space_host.db`. Two posts are created over XRPC. The real daemon runs
   against it, projects both to a capturing sink, and persists its cursor.
   The daemon is then killed and three more writes land — two creates and a
   delete — none of which it ever acknowledged.

   **Phase B — the swap.** `convert_store` converts `space_host.db` into
   per-account `store.sqlite` files. The account signing key is placed beside
   the new store, as the deploy leaves it.

   **Phase C — converged era.** The converged host starts on the same port,
   the same host database and the converted stores. The same daemon restarts
   with the same index, the same DPoP key and identical arguments. One further
   write lands at a server-minted revision.

   Finally a **cold daemon** — fresh index, fresh key, its own sink — syncs the
   converted store from scratch.

Everything lands under `target/resume/run`: a log per process
(`shim-legacy`, `shim-converged`, `daemon-legacy`, `daemon-converged`,
`daemon-cold`), the host and daemon databases, and `report.txt`. The directory
is wiped at the start of every run.

## What it asserts

- The legacy-era daemon projects what it saw, once each, and persists a cursor.
- Writes landing after its last acknowledgement leave that cursor behind.
- The conversion carries oplog ids and revisions across verbatim, and leaves
  `oplog_floor_rev` open so no `since` can be refused.
- On resume the daemon projects **exactly** the operations after its cursor,
  once each — including the three it never saw before the swap — and re-projects
  nothing.
- A revision minted by the converged host sorts after the carried legacy one,
  and its write projects once.
- No `HistoryUnavailable`, no divergence, no full-state recovery, no `prev`
  mismatch in either daemon log. This is the sharpest check: a broken cursor is
  survivable by falling back to `getRepo`, which would hide the fault behind a
  correct-looking end state.
- A cold daemon reaches the same records, revisions and LtHash digest, and the
  same projected end state.

## Credentials

All local, all fixed, all created and destroyed inside the run directory; none
of it is a secret and none of it reaches a real service.

- The gate acts as the authorization server for writes, holding the same HS256
  secret the host is configured with and signing DPoP-bound `at+jwt` tokens —
  the same shape as Layer 2.
- The daemon mints its own space credential through `/admin/mintCredential`
  with its own service identity and DPoP key, so the credential path is the
  real one and is exercised twice, once per era.
- The account's actor-store key is written by the gate; the stub DID directory
  publishes the matching `#atproto` multikey so the daemon verifies real
  commit signatures.

## Falsification

Both eras green on an unmutated tree proves nothing on its own, so the gate was
run against two deliberate converter faults:

| Mutation | Result |
|---|---|
| `oplog_floor_rev` set to the head revision instead of left open | 10/14 — the daemon took `HistoryUnavailable` → full-state recovery; the log check, the store check and both cold-sync checks went red |
| oplog row ids renumbered (`seq + 1000`) | 13/14 — only the conversion check went red |

The second is a finding, not a gap: the daemon's durable cursor is a
**revision**, not an oplog row id. Row ids are only within-request paging
cursors. Preserving them is still correct — any other consumer may hold one —
but revision continuity is what the resume depends on.

## Running outside a sandbox

Same as Layer 2: on macOS `reqwest` proxy discovery calls SystemConfiguration,
which a restricted sandbox denies and the pinned `hyper-util` panics on. Run
with ordinary process permissions. `sccache` is likewise unusable there, so
`run.sh` clears `RUSTC_WRAPPER`.
