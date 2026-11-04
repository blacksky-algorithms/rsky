<p align="center">
    <a href="https://blackskyweb.xyz">
    <img src="https://github.com/user-attachments/assets/e702e2ed-5c97-4dad-94fe-237dbfbd20c9">
    </a>
</p>
<h3 align="center">
  AT Protocol, implemented in Rust
</h3>

<div align="center">

[![Ceasefire Now](https://badge.techforpalestine.org/default)](https://techforpalestine.org/learn-more)
[![dependency status](https://deps.rs/repo/github/blacksky-algorithms/rsky/status.svg?style=flat-square)](https://deps.rs/repo/github/blacksky-algorithms/rsky)
[![Follow](https://img.shields.io/badge/Follow-%40blacksky.app-0073fa?style=flat&logo=bluesky&labelColor=%23151e27&link=https%3A%2F%2Fbsky.app%2Fprofile%2Fblacksky.app)](https://bsky.app/profile/blacksky.app)
[![License](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)
[![GitHub code size in bytes](https://img.shields.io/github/languages/code-size/blacksky-algorithms/rsky?logo=github)](https://github.com/blacksky-algorithms/rsky)
[![GitHub Repo stars](https://img.shields.io/github/stars/blacksky-algorithms/rsky?style=flat&logo=github)](https://github.com/blacksky-algorithms/rsky)
[![Backers on Open Collective](https://opencollective.com/blacksky/backers/badge.svg)](#backers)
[![Join Our Discord](https://img.shields.io/badge/Join%20Us%20on-Discord-7289DA.svg?style=for-the-badge&logo=discord)](https://discord.gg/AFuuPvM7zZ)

</div>

---

rsky (/ˈrɪski/) is a full implementation of AT Protocol in Rust. Most of the code is general purpose. Some of it, like rsky-feedgen, is written for the needs of the Blacksky community.

We developed rsky so that any community on the network can run their own infrastructure, moderate on their own terms, and leave if they ever need to. A second implementation of the protocol is how that stays possible.

> [!WARNING]
> This is a work in progress. Things change, things are incomplete, and things break. Until 1.0.0 we make no stability promises.

## What's in here

**Crates** (published to crates.io, usable on their own):

| Crate | What it does | Docs | crates.io |
|---|---|---|---|
| `rsky-common` | Shared code | [README](./rsky-common/README.md) | [![Crate](https://img.shields.io/crates/v/rsky-common?logo=rust&style=flat-square&logoColor=E05D44&color=E05D44)](https://crates.io/crates/rsky-common) |
| `rsky-crypto` | Signing and key serialization | [README](./rsky-crypto/README.md) | [![Crate](https://img.shields.io/crates/v/rsky-crypto?logo=rust&style=flat-square&logoColor=E05D44&color=E05D44)](https://crates.io/crates/rsky-crypto) |
| `rsky-identity` | DID and handle resolution | [README](./rsky-identity/README.md) | [![Crate](https://img.shields.io/crates/v/rsky-identity?logo=rust&style=flat-square&logoColor=E05D44&color=E05D44)](https://crates.io/crates/rsky-identity) |
| `rsky-lexicon` | Lexicon schema types | [README](./rsky-lexicon/README.md) | [![Crate](https://img.shields.io/crates/v/rsky-lexicon?logo=rust&style=flat-square&logoColor=E05D44&color=E05D44)](https://crates.io/crates/rsky-lexicon) |
| `rsky-oauth` | OAuth provider core. JOSE, JWK and DPoP for AT Protocol services | [README](./rsky-oauth/README.md) | [![Crate](https://img.shields.io/crates/v/rsky-oauth?logo=rust&style=flat-square&logoColor=E05D44&color=E05D44)](https://crates.io/crates/rsky-oauth) |
| `rsky-repo` | Repository storage, including the MST | [README](./rsky-repo/README.md) | [![Crate](https://img.shields.io/crates/v/rsky-repo?logo=rust&style=flat-square&logoColor=E05D44&color=E05D44)](https://crates.io/crates/rsky-repo) |
| `rsky-syntax` | Parsers for handles, DIDs, NSIDs and other identifiers | [README](./rsky-syntax/README.md) | [![Crate](https://img.shields.io/crates/v/rsky-syntax?logo=rust&style=flat-square&logoColor=E05D44&color=E05D44)](https://crates.io/crates/rsky-syntax) |
| `rsky-space` | Permissioned-data primitives. LtHash commits, credentials and space repos for access controlled or private records that only members can read | [README](./rsky-space/README.md) | [![Crate](https://img.shields.io/crates/v/rsky-space?logo=rust&style=flat-square&logoColor=E05D44&color=E05D44)](https://crates.io/crates/rsky-space) |

**Services** (the things we deploy):

- `rsky-pds`: Personal Data Server. It hosts accounts and their repos. Each account's data lives in its own SQLite store on disk, blobs go to local disk or an S3 bucket, and email goes out through Mailgun or any SMTP server.
- `rsky-pds-router`: Stateless HTTP router that sits in front of two PDS implementations sharing one data directory. It pins each account to one writer, lets reads move to the second implementation one account at a time, and journals every write before forwarding it, so accounts can migrate between implementations while both stay live.
- `rsky-pdsadmin`: Admin tooling for a running PDS.
- `rsky-relay`: Crawls the network and emits one stream of everything it finds. The "big world" piece.
- `rsky-firehose`: Firehose consumer.
- `rsky-jetstream-subscriber`: Firehose consumer for Jetstream.
- `rsky-feedgen`: Feed generator, built around the Blacksky community's feeds.
- `rsky-labeler`: Firehose consumer that labels content for moderation.
- `rsky-wintermute`: Indexer and backfiller for the app view.
- `rsky-space-host`: Authority for a permissioned space. It issues space credentials, manages the space, and routes write notifications to members.
- `rsky-daemon`: Syncer for permissioned data. It pulls each member's permissioned repo from their PDS, verifies it, and indexes it so a space can be read as one whole.
- `rsky-video`: Video processing and serving.
- `rsky-satnav`: "Structured Archive Traversal, Navigation & Verification." Explores DASL CAR files and AT Protocol repos.

**Lexicons** (`/lexicons`): Lexicon definitions for the parts of the protocol we extend, such as spaces.

## About AT Protocol

The Authenticated Transfer Protocol (atproto) is a decentralized social protocol developed by [Bluesky PBC](https://bsky.social). Start here:

- [Overview and guides](https://atproto.com/guides/overview) 👈🏾 the best starting point
- [Protocol specifications](https://atproto.com/specs/atp)
- [GitHub discussions](https://github.com/bluesky-social/atproto/discussions) 👈🏾 good place to ask questions
- [Self-authenticating data structures](https://bsky.social/about/blog/3-6-2022-a-self-authenticating-social-protocol)

## Contributing

Issues and pull requests are welcome. Before you open one:

- Read the [Code of Conduct](/.github/CODE_OF_CONDUCT.md). We hold to it.
- Follow the [canonical TypeScript implementation](https://github.com/bluesky-social/atproto) closely. Where rsky differs on purpose, the difference is documented.
- Check for an existing issue before filing a new one.
- Open an issue and give it time for discussion before you send a PR.

PRs we will close without much discussion:

- Large refactors
- New features that were not discussed first
- Changes to tooling or frameworks that were not discussed first
- New dependencies we don't need

What to expect from us: we are a small team running production infrastructure. We will try to respond, and sometimes we won't. We may close an issue or PR with little feedback, lock a thread if it's eating our attention, and we don't provide support for build issues.

## Backers

[Become a backer](https://opencollective.com/blacksky#backer) and your avatar shows up here with a link to your site.

<a href="https://opencollective.com/blacksky/backer/0/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/0/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/1/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/1/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/2/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/2/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/3/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/3/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/4/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/4/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/5/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/5/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/6/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/6/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/7/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/7/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/8/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/8/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/9/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/9/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/10/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/10/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/11/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/11/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/12/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/12/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/13/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/13/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/14/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/14/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/15/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/15/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/16/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/16/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/17/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/17/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/18/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/18/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/19/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/19/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/20/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/20/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/21/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/21/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/22/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/22/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/23/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/23/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/24/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/24/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/25/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/25/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/26/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/26/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/27/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/27/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/28/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/28/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/29/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/29/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/30/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/30/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/31/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/31/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/32/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/32/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/33/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/33/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/34/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/34/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/35/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/35/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/36/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/36/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/37/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/37/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/38/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/38/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/39/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/39/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/40/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/40/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/41/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/41/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/42/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/42/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/43/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/43/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/44/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/44/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/45/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/45/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/46/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/46/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/47/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/47/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/48/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/48/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/49/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/49/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/50/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/50/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/51/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/51/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/52/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/52/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/53/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/53/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/54/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/54/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/55/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/55/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/56/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/56/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/57/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/57/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/58/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/58/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/59/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/59/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/60/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/60/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/61/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/61/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/62/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/62/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/63/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/63/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/64/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/64/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/65/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/65/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/66/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/66/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/67/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/67/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/68/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/68/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/69/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/69/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/70/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/70/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/71/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/71/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/72/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/72/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/73/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/73/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/74/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/74/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/75/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/75/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/76/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/76/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/77/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/77/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/78/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/78/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/79/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/79/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/80/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/80/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/81/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/81/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/82/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/82/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/83/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/83/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/84/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/84/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/85/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/85/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/86/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/86/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/87/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/87/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/88/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/88/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/89/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/89/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/90/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/90/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/91/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/91/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/92/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/92/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/93/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/93/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/94/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/94/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/95/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/95/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/96/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/96/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/97/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/97/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/98/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/98/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/99/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/99/avatar.svg?requireActive=false"></a>
<a href="https://opencollective.com/blacksky/backer/100/website?requireActive=false" target="_blank"><img src="https://opencollective.com/blacksky/backer/100/avatar.svg?requireActive=false"></a>

## License

rsky is released under the [Apache License 2.0](./LICENSE).
