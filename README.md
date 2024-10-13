<p align="center">
    <a href="https://blackskyweb.xyz">
    <img src="https://cdn.prod.website-files.com/654d195a7700754d810d2693/66ef82384f93b80bfb132738_rsky-banner-2.jpg">
    </a>
</p>
<h3 align="center">
  AT Protocol Implementation (Rust)
</h3>

<div align="center">

[![Ceasefire Now](https://badge.techforpalestine.org/default)](https://techforpalestine.org/learn-more)
[![dependency status](https://deps.rs/repo/github/blacksky-algorithms/rsky/status.svg?style=flat-square)](https://deps.rs/repo/github/blacksky-algorithms/rsky)
[![Follow](https://img.shields.io/badge/Follow-%40blacksky.app-0073fa?style=flat&logo=bluesky&labelColor=%23151e27&link=https%3A%2F%2Fbsky.app%2Fprofile%2Fblacksky.app)](https://bsky.app/profile/blacksky.app)
[![License](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)
[![GitHub code size in bytes](https://img.shields.io/github/languages/code-size/blacksky-algorithms/rsky?logo=github)](https://github.com/blacksky-algorithms/rsky)
[![GitHub Repo stars](https://img.shields.io/github/stars/blacksky-algorithms/rsky?style=flat&logo=github)](https://github.com/blacksky-algorithms/rsky)
[![Backers on Open Collective](https://opencollective.com/blacksky/backers/badge.svg)](#backers)
[![Sponsors on Open Collective](https://opencollective.com/blacksky/sponsors/badge.svg)](#sponsors)

</div>

---
> [!WARNING]
> ***This library is a work in progress. Things will change. Things are incomplete. Things will break. Until the project reaches version 1.0.0, stability will not be guaranteed.***

rsky (/ˈrɪski/) is intended to be a full implementation of [AT Protocol](https://atproto.com/) in the Rust language. Most of the code here are general purpose implementations while some (like rsky-feedgen) are specific to the use cases of the Blacksky community.

## What is in here?

**Rust Crates:**

| Crate                                                                       | Docs                                       | crates.io                                                                                                             |
| ----------------------------------------------------------------------------- | ------------------------------------------ | --------------------------------------------------------------------------------------------------------------- |
| `rsky-crypto`: cryptographic signing and key serialization                | [README](./rsky-crypto/README.md)      | [![Crate](https://img.shields.io/crates/v/rsky-crypto?logo=rust&style=flat-square&logoColor=E05D44&color=E05D44)](https://crates.io/crates/rsky-crypto)          |
| `rsky-identity`: DID and handle resolution                                | [README](./rsky-identity/README.md)    | [![Crate](https://img.shields.io/crates/v/rsky-identity?logo=rust&style=flat-square&logoColor=E05D44&color=E05D44)](https://crates.io/crates/rsky-identity)       |
| `rsky-lexicon`: schema definition language                                | [README](./rsky-lexicon/README.md)     | [![Crate](https://img.shields.io/crates/v/rsky-lexicon?logo=rust&style=flat-square&logoColor=E05D44&color=E05D44)](https://crates.io/crates/rsky-lexicon)         |
| `rsky-syntax`: string parsers for identifiers                             | [README](./rsky-syntax/README.md)    | [![Crate](https://img.shields.io/crates/v/rsky-syntax?logo=rust&style=flat-square&logoColor=E05D44&color=E05D44)](https://crates.io/crates/rsky-syntax)          |

**Rust Services:**

- `rsky-pds`: "Personal Data Server", hosting repo content for atproto accounts. It differs from the canonical Typescript implementation by using Postgres instead of SQLite, s3 compatible blob storage instead of on-disk, and mailgun for emailing. All to make the PDS easier to migrate between cloud hosting providers and more maintainable.
- `rsky-feedgen`: Bluesky feed generator that closely follows the use cases of the Blacksky community.
- `rsky-firehose`: Firehose consumer.

## About AT Protocol

The Authenticated Transfer Protocol ("ATP" or "atproto") is a decentralized social media protocol, developed by [Bluesky PBC](https://bsky.social). Learn more at:

- [Overview and Guides](https://atproto.com/guides/overview) 👈🏾 Best starting point
- [Github Discussions](https://github.com/bluesky-social/atproto/discussions) 👈🏾 Great place to ask questions
- [Protocol Specifications](https://atproto.com/specs/atp)
- [Blogpost on self-authenticating data structures](https://bsky.social/about/blog/3-6-2022-a-self-authenticating-social-protocol)

## Roadmap

-   [x] Feedgen and firehose consumer
-   [x] PDS implementation
-   [ ] Frontend bluesky client
-   [ ] Feedgen admin client

## Backers

[Become a backer](https://opencollective.com/blacksky#backer) and get your image on our README on GitHub with a link to your site.

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

## Contribution

We welcome contributions from the community to help us improve and expand rsky. If you're interested in contributing, please feel free to submit issues or pull requests on the GitHub repository. We appreciate your support!

**Rules:**

- We'll try our best but may not respond to your issue or PR.
- We may close an issue or PR without much feedback.
- We may lock discussions or contributions if our attention is getting DDOSed.
- We do not provide support for build issues.

**Guidelines:**

- Strict adherence to our [Code of Conduct](/.github/CODE_OF_CONDUCT.md)
- Implementations should follow closely to the [canonical Typescript implementation](https://github.com/bluesky-social/atproto)
- Check for existing issues before filing a new one, please.
- Open an issue and give some time for discussion before submitting a PR.
- Stay away from PRs that:
    - Refactor large parts of the codebase
    - Add entirely new features without prior discussion
    - Change the tooling or frameworks used without prior discussion
    - Introduce new unnecessary dependencies

## License

rsky is released under the [Apache License 2.0](./LICENSE).