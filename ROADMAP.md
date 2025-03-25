# rsky Roadmap to v1.0.0

Welcome to the official roadmap for the `rsky` project, a full-featured implementation of the AT Protocol in Rust, developed by [Blacksky Algorithms](https://blackskyweb.xyz). Our goal is to reach a stable, production-ready v1.0.0 release within the next year.

This document outlines the major milestones and components required to achieve that goal. Contributions, feature requests, and PRs aligned with these priorities are highly encouraged.

---

## Core Components and Priorities

### rsky-pds
A robust and reliable Personal Data Server is the foundation of the project.

**Milestones:**
- [ ] **Stability & Testing:**
  - Extensive stress testing to validate data integrity
  - Ensure no repo corruption under realistic production loads
- [ ] **Blob Storage Support:**
  - Support for multiple S3-compatible backends (e.g., AWS, DigitalOcean)
  - Reliable on-disk blob and repo storage fallback option
- [ ] **OAuth & Authentication:**
  - Implement server-side OAuth flows including DPoP
  - Integrate support for scoped auth and private state (as finalized by Bluesky)
- [ ] **User-Facing Web Pages:**
  - Simple OAuth approval pages
  - Generic user signup and account management UI served directly by the PDS
- [ ] **Future-Proofing:**
  - Evaluate role of PDS in potential E2EE chat workflows

---

### rsky-relay
A high-throughput relay service that ingests and republishes AT Protocol data across the network.

**Milestones:**
- [ ] **New Crates:**
  - `rsky-carstore`: Efficient storage and indexing of CAR files
  - Additional crates to manage indexing, subscriptions, and filtering
- [ ] **Relay Design:**
  - Support full-network ingestion at high bandwidth
  - Prioritize performance, scalability, and observability

---
### rsky-jetstream
Jetstream is a streaming service that consumes the AT Protocol firehose (`com.atproto.sync.subscribeRepos`) and converts the CBOR-encoded MST blocks into lightweight, developer-friendly JSON.

**Milestones:**
- [ ] Implement a Jetstream-compatible service that:
  - Subscribes to the full firehose stream
  - Parses and decodes CBOR-encoded data structures (including MST blocks)
  - Outputs clean JSON events for easy use in downstream tooling
- [ ] Consider various developer use cases like backlink filtering

---

### App-View Layer

**Milestones:**
- [ ] Implement or integrate an **App-View** (e.g., Cypher or custom)
- [ ] Contribute to network resilience via specialized services:
  - Backlink aggregation (e.g., total number of non-deleted likes, reposts, quotes)
  - CDN or media service for blob caching and transcoding

---

### Documentation

**Milestones:**
- [ ] Comprehensive READMEs for each crate
- [ ] Developer guides and architecture references at [docs.blackskyweb.xyz](https://docs.blackskyweb.xyz)
- [ ] Tutorials, examples, and API documentation

---

### Production Readiness

**Milestones:**
- [ ] Improved dependency management across rsky workspace members through dependabot, `[cargo machete](https://github.com/bnjbvr/cargo-machete)`, version management, etc.
- [ ] Adding telemetry in the services mentioned above to yield observability
- [ ] Incorporating [12-factor app](https://12factor.net/) principles that will allow rsky users to build scalable, maintainable ATProto services that follow modern cloud-native best practices.

---

## Timeline
We expect to hit v1.0.0 within **12 months** from now. Progress will be community-driven and will evolve alongside changes to the AT Protocol and the needs of Blacksky-hosted communities.

## Visual Roadmap (Quarterly Breakdown)

| Component        | Q2 2025                        | Q3 2025                             | Q4 2025                          | Q1 2026                        |
|------------------|--------------------------------|-------------------------------------|----------------------------------|--------------------------------|
| **rsky-pds**     | Testing, S3/On-Disk Blob Storage | OAuth, Web UI         | Scoped Auth + Private State Impl. | Evaluate E2EE Chat Integration |
| **rsky-relay**   | Crate Setup (`carstore`, etc)  | High-throughput relay implementation | Indexing, Filtering, Scaling     | Production Stabilization       |
| **rsky-jetstream** | Exploration | Implementation  | Testing       | Polishing, performance tuning  |
| **App-View**     | Evaluate Cypher, Plan Aggregators | Build backlink/CDN services         | Testing custom services          | Deploy App-View components     |
| **Documentation**| Improve READMEs, crate-level docs | Dev guides, architecture docs       | API docs, examples               | Tutorials, onboarding flows    |
| **Production Readiness** | Version management, dependency tracking | Telemetry implementation, observability foundation | 12-Factor configuration and environment setup | Complete 12-Factor principles |

---

## How to Contribute
We're building this in the open. We encourage:
- Feature requests
- Bug reports
- Pull requests
- Testing and benchmarking

Start with issues labeled `roadmap` or `v1-priority`.

This document is subject to change as protocol development progresses and as community needs evolve.

---

**Project Repository:** https://github.com/blacksky-algorithms/rsky  
**Docs:** https://docs.blackskyweb.xyz