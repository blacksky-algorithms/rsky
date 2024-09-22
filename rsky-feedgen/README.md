# <h1> rsky-feedgen </h1>


A Bluesky feed generator that closely follows the use cases of the Blacksky community. It includes an API for receiving records from a firehose subscriber.

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
-   [x] Queuable database operations
-   [x] API-token security
-   [x] Visitor tracking
-   [x] Hashtag filtering
-   [x] Members database
-   [x] All posts, trending, and language filters

## Credits

This project would not have been possible without the great work done in:

-   [`feed-gen`](https://github.com/bluesky-social/feed-generator)
-   [`bisky`](https://github.com/jesopo/bisky)
-   [`algoz`](https://github.com/whyrusleeping/algoz)

A lot of the code was inspired and adapted from their work.
