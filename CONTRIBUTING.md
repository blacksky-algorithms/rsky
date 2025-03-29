# Blacksky Algorithms' Contributor Guide

---
First, we appreciate you showing interest in contributing to the rsky project! Any form of contribution is appreciated.
In this document, we outline the different ways one can contribute to this project.

Second, understand that all repositories in the **rsky** repository fall under two categories,
"crates", and "services". Crates are libraries for hosted on the [Crates website](https://crates.io/search?q=rsky).
The repositories in the "services" section are what is being used to serve the end user. 

---

## Getting Started
This project heavily involves the AT Protocol. There are resources attached to this
document that you can use to gain a better understanding.

At a high level, the **Authenticated Transfer Protocol** (aka AT Protocol, ATProto, atproto) is a generic, federated protocol for
building open social media appliactions. **Personal Data Stores** (or PDSs) store user data
and handles identity. **Relays** aggregate and distributes data across the network. **App Views** aggregate data from
the relays, for it to be used in feeds.

### Resources
* [ATProto for Distributed Systems Engineers](https://atproto.com/articles/atproto-for-distsys-engineers)
* [AT Protocol Specifications](https://atproto.com/#resources)
* [Official Bluesky PDS (In TypeScript)](https://github.com/bluesky-social/atproto/tree/main/packages/pds)
* [Official Blacksky PDS (In Rust)](https://github.com/blacksky-algorithms/rsky/tree/main/rsky-pds)
* [Bigsky, the official Bluesky Relay](https://github.com/bluesky-social/indigo/tree/main/cmd/bigsky)
  * It can be accessed at https://relay1.us-east.bsky.network, but the popular option is to use https://bsky.network/.
* [Jetstream, a bandwidth-friendly relay](https://github.com/bluesky-social/jetstream)
* [Official Bluesky Appview](https://github.com/bluesky-social/atproto/tree/main/packages/bsky)
* [Example AppView](https://github.com/bluesky-social/statusphere-example-app/tree/main)
* [Constellation, a global backlink indexing tool](https://github.com/at-microcosm/links/tree/main/constellation)


## Contribution Process
### Submitting a Bug Report
Before you decide that it's time to submit a bug report, be sure to investigate the issue, and thoroughly read
the documentation. Before submitting any report, please go through the checklist below to help us fix this bug
as soon as possible.
- Confirm that you are using the latest version of the project
- Make sure the issue you are experiencing isn't due to any user error.
- Check if there is an existing issue outlining the same one you are experiencing. If you do happen to find one,
consider leaving a comment with your experience.

In the report, we ask that you include the following information: 
- Any/All error logs
- The operating system, and platform you were using (E.g. Linux x86_64)
- The Rust Compiler Version (type `rustc --version` to get the version)
- If you are able to reliably reproduce this issue, how.

If your bug report is concerning a security vulnerability, we encourage you to email us at <provide-email>.

### Submitting a Feature Request
Similar to the [Bug Report](#submitting-a-bug-report) section, confirm that you are using the latest version, and that
you have read through the documentation. Additionally, make sure the idea is within the scope of the project. Ask
yourself if this feature will only be useful to the majority of users, or if there will only be a subsection of people
that will benefit. If it is the latter option, consider making an add-on library with that functionality. Finally,
the feature request has not already been suggested. If it has, be sure to add a comment or a reaction.

## Code of Conduct
This project and everyone participating in it is governed by the [Code of Conduct](https://github.com/blacksky-algorithms/rsky/blob/main/.github/CODE_OF_CONDUCT.md). 
By participating, you are expected to uphold this code. Please report any unacceptable behavior to <get-email>
## Contact Information


## Important Links
* [License Information](https://github.com/blacksky-algorithms/rsky/blob/main/LICENSE)
* [Project Roadmap](https://github.com/d3ol-dev/rsky/blob/main/ROADMAP.md)
* [Issue Tracker](https://github.com/blacksky-algorithms/rsky/issues)
