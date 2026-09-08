# Vendored: hubble-sync 0.0.4

Copied from https://tangled.org/microcosm.blue/hubble at commit `d984a19`
("bump versions for release", 2026-08-07), crate `hubble-sync`, which is the
source of the crates.io release 0.0.4. Copyright (c) 2025 microcosm.
Licensed under MIT OR Apache-2.0 (LICENSE.MIT and LICENSE.Apache-2.0 in this
directory); the rsky workspace is Apache-2.0, so it is used here under the
Apache-2.0 option.

## Changes from upstream

- `Cargo.toml`: workspace dependency references replaced with the concrete
  versions from hubble's workspace manifest at that commit; `publish = false`.
- `src/lib.rs`: additional public re-exports so an external crate can
  implement `SyncConsumer::resync` (per-repo fetch routing) and seed repos
  from an external DID list:
  `Resyncable`, `ResyncError`, `TransientResyncError`, `load_repo`,
  `BigRepoPermits`, `RepoDiscovery`, `DiscoveredRepo`, `RepoIdentity`,
  `ResolvedIdentity`, `ResolutionError`.
- `src/hubble_sync.rs`: `HubbleSync::discovery()` accessor for the intake used
  to request syncs for externally discovered DIDs.

Everything else is byte-for-byte upstream. Upstream is the source of truth;
the intent is to send these re-exports upstream and delete this copy.
