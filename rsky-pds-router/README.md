# rsky-pds-router

A stateless HTTP router placed in front of two PDS implementations that share
one data directory. It decides, per request, which process answers: the
account a request concerns is pinned to one writer at a time, reads can be
staged to the second implementation account by account, and every mutation
is recorded in a write-ahead journal before it is forwarded.

The router never rewrites request bodies and never retries a mutation.

## How requests are routed

Every request is classified by method, path, query, and headers:

- `GET /xrpc/_health` is answered by the router itself.
- Authorization-server traffic (`/oauth/*`, `/.well-known/oauth-*`, the
  provider's assets, and the read-only or consent endpoints of its UI API)
  passes through to `ROUTER_UPSTREAM_OAUTH`.
- Reads (`GET`, `HEAD`, `OPTIONS`) belong to one of three pools: `sync`
  (`com.atproto.sync.*`), `bsky` (`app.bsky.*`, `chat.bsky.*`), or `main`
  (everything else). The account a read concerns is taken from the `did`,
  `repo`, or `actor` query parameter, from the `Host` header for well-known
  lookups, or from the bearer token's `sub` claim. If that account is pinned
  to rsky (or reads default to rsky and it is not held back) the read goes to
  rsky; otherwise to the TypeScript pool for its class.
- Mutations are looked up in the inventory (`src/inventory.rs`), which lists
  every mutating XRPC procedure of the pinned lexicon set and every OAuth UI
  API endpoint together with a rule for finding the account it mutates
  (`body.repo`, the bearer subject, `body.did`, an email token resolved
  through the account database, and so on). A mutation the inventory does not
  know is refused with `503 RouterUnknownMutation`; a build fails if a
  procedure from `procedures-0.5.27.txt` is missing from the table.

A mutation's targets decide its backend:

| Condition | Result |
|---|---|
| a target is in `writes.canary_fence` | `503 RouterFenced` |
| a target is in `writes.canary_rsky` and `kill_switch` is set | `503 RouterKillSwitch` |
| a target is in `writes.canary_rsky` and the allowlist entry is not `active` | `503 RouterNotAdmitted` |
| a target is in `writes.canary_rsky` and the request is an OAuth UI API endpoint | `503 RouterNoEquivalent` |
| targets are split across writers | `503 RouterSplitTargets` |
| a target is a canary | rsky |
| the request cannot be attributed and any canary exists | `503 RouterUnattributable` |
| otherwise | `writes.default` (`uploadBlob` and `importRepo` go to the sync worker) |

A required target that is missing from the body is a schema violation and is
answered `400 InvalidRequest`. Bodies of JSON mutations are buffered up to 1
MiB to find their target; raw uploads are streamed untouched.

Every response carries `X-Pds-Backend` (`ts`, `rsky`, `oauth`, or `router`)
and `X-Router-Reason`.

## Failover

A read whose upstream refuses the connection is sent once more to the
TypeScript pool for its class and stamped `X-Router-Reason: failover`.
Nothing else is retried: an upstream that accepted the connection but timed
out yields `504 UpstreamTimeout`, and a mutation whose upstream is
unreachable yields `503 UpstreamUnavailable` with `Retry-After: 1`.

## Policy and allowlist

Both files are re-read within two seconds of a change; a file that fails to
parse leaves the previous value in force and logs the error.

`policy.toml`:

```toml
version = 1
kill_switch = false          # all reads to TS; canary mutations refused

[reads]
default = "ts"               # or "rsky"
pin_rsky = []                # DIDs read from rsky regardless of the default
pin_ts = []                  # DIDs held on TS when the default is rsky

[writes]
default = "ts"
canary_rsky = []             # DIDs whose mutations go to rsky or are refused
canary_fence = []            # DIDs whose mutations are refused everywhere
```

`write-allowlist.toml` mirrors the file the rsky process enforces, so a
canary's mutation is forwarded only when that process will admit it:

```toml
version = 1
default = "absent"           # or "active"

[entries]
"did:plc:example" = "active"                 # active | draining | absent
"did:plc:other" = { state = "maintenance", workflow_id = "w-1" }
```

## Write-ahead journal

Before the first byte of a mutation is forwarded the router appends a line to
its journal and syncs it; after the upstream answered it appends a second
line with the same `id`:

```json
{"t":"1757000000.123","id":0,"phase":"start","nsid":"com.atproto.repo.createRecord","did":"did:plc:example","backend":"ts"}
{"t":"1757000000.456","id":0,"phase":"end","status":200}
```

A mutation the upstream never answered (a timeout, or a connection that
failed after the request was sent) may still complete there, so its
second line is `"phase":"ambiguous"` instead of `end`; the audit keeps
such an account dirty until the TypeScript processes have been observed
quiescent after that line. A mutation whose account lookup fails is
refused with `503 RouterLookupUnavailable` rather than routed as if it
named no account.

If the journal cannot be written the mutation is refused with
`503 RouterJournalUnavailable` and `router_journal_failures_total` is
incremented. Each router instance owns one journal file; the default is
`mutations-<port>.jsonl` beside the policy file.

## Configuration

| Flag | Environment | Default |
|---|---|---|
| `--port` | `ROUTER_PORT` | `4100` |
| `--metrics-port` | `ROUTER_METRICS_PORT` | `4101` |
| `--ts-main` | `ROUTER_UPSTREAM_TS_MAIN` | `http://127.0.0.1:3000` |
| `--ts-sync` | `ROUTER_UPSTREAM_TS_SYNC` | `http://127.0.0.1:3001` |
| `--ts-read` | `ROUTER_UPSTREAM_TS_READ` | `http://127.0.0.1:3002,http://127.0.0.1:3003` |
| `--rsky` | `ROUTER_UPSTREAM_RSKY` | `http://127.0.0.1:4000` |
| `--oauth` | `ROUTER_UPSTREAM_OAUTH` | `http://127.0.0.1:3000` |
| `--policy-file` | `ROUTER_POLICY_FILE` | `/pds/router/policy.toml` |
| `--allowlist-file` | `ROUTER_ALLOWLIST_FILE` | `/pds/router/write-allowlist.toml` |
| `--journal-file` | `ROUTER_JOURNAL_FILE` | `<policy dir>/mutations-<port>.jsonl` |
| `--account-db` | `ROUTER_ACCOUNT_DB` | `/pds/account.sqlite` (opened read-only) |
| `--read-timeout-secs` | `ROUTER_READ_TIMEOUT_SECS` | `25` |
| `--sync-timeout-secs` | `ROUTER_SYNC_TIMEOUT_SECS` | `55` |
| `--write-timeout-secs` | `ROUTER_WRITE_TIMEOUT_SECS` | `300` |

Both listeners bind loopback only.

## Metrics

Served on the metrics port at `/metrics`:

- `router_requests_total{backend,class,status}`
- `router_request_duration_seconds{backend,class}`
- `router_writes_rejected_total{reason}`
- `router_journal_failures_total`
- `router_misroute_total`: mutations that reached the TypeScript process for
  an account that became a canary or was fenced while the request was in
  flight
- `router_failovers_total{class}`

## Development

```bash
cargo test -p rsky-pds-router
cargo clippy -p rsky-pds-router --all-targets
cargo llvm-cov -p rsky-pds-router
```

The integration tests in `tests/routing_tests.rs` run the router against
mock upstreams on loopback and need no external services.
