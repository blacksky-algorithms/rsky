# <h1> Rsky ☣️ ☠️ </h1>

<p><strong>An ATProtocol Feed Generator framework, built in Rust.</strong></p>

[![dependency status](https://deps.rs/repo/github/rudyfraser/rsky/status.svg?style=flat-square)](https://deps.rs/repo/github/rudyfraser/rsky) [![License](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)

This is a starter kit for creating ATProto Feed Generators. It's not feature complete, but should give you a good starting ground off of which to build and deploy a feed. It is designed as 2 binaries: a firehose subscriber and an HTTP server for returning skeletons.

## Overview

Feed Generators are services that provide custom algorithms to users through the AT Protocol.

They work very simply: the server receives a request from a user's server and returns a list of [post URIs](https://atproto.com/specs/at-uri-scheme) with some optional metadata attached. Those posts are then hydrated into full views by the requesting server and sent back to the client. This route is described in the [`app.bsky.feed.getFeedSkeleton` lexicon](https://atproto.com/lexicons/app-bsky-feed#appbskyfeedgetfeedskeleton).

A Feed Generator service can host one or more algorithms. The service itself is identified by DID, while each algorithm that it hosts is declared by a record in the repo of the account that created it. For instance, feeds offered by Bluesky will likely be declared in `@bsky.app`'s repo. Therefore, a given algorithm is identified by the at-uri of the declaration record. This declaration record includes a pointer to the service's DID along with some profile information for the feed.

The general flow of providing a custom algorithm to a user is as follows:
- A user requests a feed from their server (PDS) using the at-uri of the declared feed
- The PDS resolves the at-uri and finds the DID doc of the Feed Generator
- The PDS sends a `getFeedSkeleton` request to the service endpoint declared in the Feed Generator's DID doc
  - This request is authenticated by a JWT signed by the user's repo signing key
- The Feed Generator returns a skeleton of the feed to the user's PDS
- The PDS hydrates the feed (user info, post contents, aggregates, etc.)
  - In the future, the PDS will hydrate the feed with the help of an App View, but for now, the PDS handles hydration itself
- The PDS returns the hydrated feed to the user

For users, this should feel like visiting a page in the app. Once they subscribe to a custom algorithm, it will appear in their home interface as one of their available feeds.

## Features

-   [x] HTTP server for returning skeletons
-   [x] Firehose subscriber
-   [x] Lexicon definitions for Posts
-   [x] Queuable database operations
-   [x] serde_cbor for deserializing DAG-CBOR
-   [x] API-token security
-   [x] Visitor tracking
-   [x] Image decoding
-   [x] NSFW image classification


## User Warning

This project comes as is. We provide no guarantee of stability or support, as the crates closely follow the needs of the [`BlackSky`](https://bsky.app/profile/did:plc:w4xbfzo7kqfes5zb7r6qv3rw/feed/blacksky/) project.

If you use this project in a production environment, it is your responsibility to perform a security audit to ensure that the software meets your requirements.


## Credits

This project would not have been possible without the great work done in:

-   [`feed-gen`](https://github.com/bluesky-social/feed-generator)
-   [`bisky`](https://github.com/jesopo/bisky)
-   [`algoz`](https://github.com/whyrusleeping/algoz)

A lot of the code was inspired and adapted from their work.
