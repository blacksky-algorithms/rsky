# rsky-daemon: syncer for ATProto permissioned data spaces

**rsky-daemon** is a **syncer** for ATProto permissioned data.

**rsky-daemon** is a dedicated service, and not part of the AppView, because **permissioned data space** is a protocol parallel to *public ATProto*.
The primitives it builds on are in [`rsky-space`](../rsky-space/README.md).

## ATProto

**ATProto** (which is short for **AT-Protocol**, which is itself short for **Authenticated Transfer Protocol**) is a protocol commonly used for decentralized social media — but, is in general a **social web** protocol (comparable to ActivityPub).
ATProto is a modular, federated networking stack designed to decouple user identity and data storage from the application layer.
Under the hood, it combines cryptographic identifiers (DIDs), signed Merkle search trees (MSTs) hosted on Personal Data Servers (PDSs), and schema-driven records defined via Lexicon to ensure data portability and verifiable integrity.
For a developer, this means you can build interoperable social applications where users truly own their data and social graphs, without reinventing identity management or custom backend infrastructure.
Users publish by writing records to their own PDS, and relays combine the updates from every PDS into a firehose, a WebSocket event stream that applications consume.
That firehose carries only public data; permissioned data, below, has none.

The common ATProto services are:

- **PDS** (Personal Data Server): hosts users' accounts and repos, and is where apps write records on a user's behalf.
- **PLC directory** (`plc.directory`): resolves `did:plc` identifiers to DID documents, which name each user's PDS and signing key.
- **Relay**: crawls PDSes and combines their updates into the firehose.
- **AppView**: indexes the firehose to serve an application's views, such as timelines, threads, and like counts.
- **Feed generator**: supplies custom feeds, as lists of posts that the AppView fills in.
- **Labeler**: publishes moderation labels on accounts and content.

## ATProto Permissioned Data Spaces

**ATProto Permissioned Data Spaces** (or just **spaces**) is an additional protocol for **permissioned data** in ATProto — data with an access perimeter, as opposed to the public data described above.
A **space** is an authorization and sync boundary for a shared social context, such as a private forum, a community, a group chat, subscriber-only posts, or a user's own bookmarks and drafts.
Each member's records for a space live in a per-user **space repo** on that member's own PDS, and reading them requires a short-lived **space credential** issued on behalf of the space's authority.
Spaces keep what public ATProto has — DID-based identity, per-user repositories, and Lexicon-typed records — but have their own repository format, sync mechanism, addressing scheme, and resolution path.
They provide access control, not confidentiality: space data is not end-to-end encrypted, so the hosts and syncers that serve it can read it.
The design is specified in Bluesky's [proposal 0016](https://github.com/bluesky-social/proposals/blob/main/0016-permissioned-data/README.md), which is still a proposal with alpha implementations rather than a final specification.

Unlike a public repo, a space repo is not an MST: it is summarized by an LtHash set hash, which does not depend on the order its records were written in.
Its commits are deniable: the signature covers only random per-commit bytes, and a MAC binds the commit to the records, so a copy that leaks outside the space proves nothing about what the author wrote.

The permissioned data services are:

- **Space authority**: the DID at the root of a space, named in its URI (`at://{authority}/space/…`); it is an identity, not a server.
- **Space host**: answers for a space as a whole: it issues space credentials, lists the members who have written to the space, and routes write notifications (in rsky: [`rsky-space-host`](../rsky-space-host)).
- **Repo host**: stores and serves each member's permissioned repo, usually the member's own PDS (in rsky: [`rsky-pds`](../rsky-pds/README.md)).
- **Syncer**: keeps its own copy of a space by pulling each member's repo from its repo host (in rsky: this crate).
- **Managing app**: the application a space belongs to; it can decide who may read the space and tell syncers which spaces exist.

There is no relay: each syncer pulls from the repo hosts itself.

## ATProto Permissioned Data Spaces Syncer

A **syncer** is any application that keeps its own copy of a space, pulling each member's space repo directly from that member's repo host.
It is the permissioned-data counterpart of an AppView reading the firehose: with no relay, the syncer goes to the data itself.
To read a space, a syncer holds a short-lived space credential bound to a key it controls, and renews it before it expires; letting credentials lapse is how access is revoked.
The space host tells the syncer which members have written to the space, and notifies it when one of them writes again.
For each member, the syncer applies the repo's new operations to its copy and checks the result against the repo's commit; when they disagree, or the history it needs is gone, it fetches the whole repo again.
When a space is deleted, the syncer is expected to delete every copy of that space's data it holds.

Being a syncer is a role, not a kind of server: an app's backend, an AppView, or a moderation service can each be one.
**rsky-daemon** is Blacksky's syncer, run as a standalone service: it indexes the spaces it follows and projects their records to a feed service and an AppView.

## How it syncs

- **Discovery**: the spaces to follow come from `--space-uri`, from a managing app's space list (`--spaces-url`), or both.
- **Notifications**: the daemon registers its endpoint with each space host and syncs a repo as soon as a `notifyWrite` arrives.
- **Sweeps**: every `--sweep-interval-secs` (default 300) it walks each space's writer set, so a dropped notification is caught on the next sweep.
- **Recovery**: a repo is synced incrementally from its op log.
  When that cannot proceed (the hash diverged, history was compacted, or the host served an unsigned commit) the daemon fetches the whole repo as a CAR instead.

## Credentials

Reading a space takes a short-lived space credential, bound to the daemon's DPoP key (`--dpop-key-path`).
There are three ways to get one, and exactly one must be configured:

| Mode           | Settings                                               | Notes                                                                   |
|----------------|--------------------------------------------------------|-------------------------------------------------------------------------|
| Acorn Auth     | `--auth-url`, `--auth-key`                             | Auth issues a delegation token per space, exchanged with the space host |
| Legacy minting | `--space-host-mint-token`, `--service-signing-key-hex` | Minted by the space host's `/admin/mintCredential`                      |
| Static         | `--static-credential`                                  | Development only: never renewed, so it stops working when it expires    |

## Running

```sh
cargo run --release -p rsky-daemon -- --help
```

Every option can also be set by an environment variable named `DAEMON_*`; `--help` lists each one with its default.
Always required:

- `--space-host-url`: the space host (authority).
- `--service-identity`: the daemon's DID, usually `did:web:<host>`.
- `--dpop-key-path`: the DPoP key file, created on first run.
- `--space-uri` or `--spaces-url` (with `--spaces-api-key`).
- One credential mode from the table above.

`--service-signing-key-hex` is also needed to serve the daemon's DID document and to project records (`--feeds-url`, `--appview-url`).

State is the SQLite index (`--index-db-path`; empty keeps it in memory, for development only) and the DPoP key file.
Keep both on persistent storage.

The [`Dockerfile`](./Dockerfile) builds the binary from the repo root and binds the listener to `0.0.0.0:8055`.

## HTTP endpoints

Served on `--notify-bind` (default `127.0.0.1:8055`):

| Path                                         | Purpose                                                     |
|----------------------------------------------|-------------------------------------------------------------|
| `/.well-known/did.json`                      | The daemon's `did:web` document, naming its notify endpoint |
| `/xrpc/com.atproto.space.notifyWrite`        | Write notifications from repo hosts                         |
| `/xrpc/com.atproto.space.notifySpaceDeleted` | Space deletion notices                                      |
| `/internal/v1/spaces/refresh`                | Re-run space discovery; enabled by `--refresh-issuer-did`   |
| `/metrics`                                   | Prometheus metrics (`daemon_*`)                             |

## License

rsky is released under the [Apache License 2.0](../LICENSE).
