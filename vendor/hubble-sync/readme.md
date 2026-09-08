# hubble-sync: atproto sync1.1 core

possibly-ok low-level sync library in rust on synchronous embedded storage with pretty-alright consistency


**you give:**

- a function for applying a (validated, resolved) commit to your data
- a function for applying a (fetched and parsed) repo resync to your data
- a function for applying a repository status change
- a reference to a fjall, rocksdb, or anything implementing the `Storage` trait

(optionally: an override of the repo-fetch step of a resync, if you're doing weird tricks like lightrail does)


**you get:**

- fast, resource-bounded, full-network backfill
- firehose pre-processing with transparent transition to desynchronized and resync scheduling
- identity resolution without cold-cache startup surge
- a `PdsHost` type that maintains ratelimits and stuff
- strict atproto sync1.1 compliance, with (degraded) pre-sync1.1 compatibility


**but not:**

- collection signalling: full network or nothing.
  - likely to be added later if this thing is useful.

- record storage: consider Hydrant if you want your sync tool to store records for you.
  - never going to be part of hubble-sync.

- record set reconciliation: consider Tap if you want per-record add/delete/removes emitted on resync.
  - hubble-sync gives you the entire new repo to reconcile against your own state.
  - could be built as a higher-level library on top of hubble-sync.


## (aspirational) quickstart

(not yet runnable)

```rust
use std::sync::Arc;
use hubble_sync::{AppResult, HubbleSync, SyncConsumer, Repo, Resync, Commit, Status};
use hubble_sync_rocksdb::RocksEngine;
use rocksdb::{DB, Options as RocksOpts, WriteBatch};

/// demo app: track how many bluesky likes each user has
struct LikesCounter {
    /// the *same* database instance HubbleSync will use
    db: Arc<DB>,
    /// in this example we're using rocksdb apis directly -- HubbleSync should
    /// eventually get some non-internal basic key-value apis exposed, so you
    /// only need to drop down to the concrete engine for engine-specific needs
    points_cf: &'static str,
}

/// your app-specific stuff! take care with blocking db i/o from your own code.
impl LikesCounter {
    /// use a prefix (likes/) to avoid keyspace conflicts with HubbleSync (or
    /// create your own ColumnFamily, which costs mem+maintenance overhead)
    fn key_for_user(did: &str) -> Vec<u8> {
        format!("likes/{did}").into_bytes()
    }

    /// commit => adjust existing value
    ///
    /// note! this read-outside-of-batch is actually safe with HubbleSync!
    /// all incoming events are serialized per-repo, so there is no other writer
    /// touching this key in between. (unless your own app does, separately)
    fn adjust_likes_for_user(
        &self,
        batch: &mut WriteBatch,
        did: &str,
        change: i32,
    ) -> AppResult<RocksEngine, ()> {
        let cf = self.db.cf_handle(self.points_cf).unwrap();
        let Some(db_val) = self.db.get_cf(&cf, &Self::key_for_user(did))? else {
            eprintln!("bug: cannot adjust likes for a user we haven't synced");
            return Ok(())
        };
        let current = u32::from_be_bytes(db_val.try_into()?);
        let adjusted = current + change; // todo: error on overflows, etc
        batch.put_cf(&cf, &Self::key_for_user(did), &adjusted.to_be_bytes())
    }

    /// resync => a new ground-truth (for this repo)
    ///
    /// note! writes here are concurrency-safe, because all per-repo handling is
    /// serialized at the application layer by HubbleSync.
    fn set_likes_for_user(
        &self,
        batch: &mut WriteBatch,
        did: &str,
        likes: u32,
    ) -> AppResult<RocksEngine, ()> {
        let cf = self.db.cf_handle(self.points_cf).unwrap();
        batch.put_cf(&cf, &Self::key_for_user(did), &likes.to_be_bytes())
    }
}

/// your app's connection into the HubbleSync machinery!
///
/// this is it!
impl SyncConsumer for LikesCounter {
    type Engine = RocksEngine;

    /// sync-1.1-verified synchronized commit
    fn apply_commit(
        &self,
        repo: &Repo,
        commit: &Commit,
        batch: &mut Self::Engine::Batch,
    ) -> AppResult<Self::Engine, ()> {
        let is_like = |(k, _, _)| k.starts_with("app.bsky.feed.like/");
        let likes_added = commit.added().iter().filter(is_like).count() as i32;
        let likes_removed = commit.deleted().iter().filter(is_like).count() as i32;
        let change = likes_added - likes_removed;
        self.adjust_likes_for_user(batch.inner_mut(), repo.did(), change)?;
        Ok(())
    }

    /// a full resync happened (backfill or repair after desync)
    fn apply_resync(
        &self,
        repo: &Repo,
        resync_data: Resync,
        batch: &mut Self::Engine::Batch,
    ) -> Result<Self::Engine, ()> {
        let mut likes = 0;
        let like_records = resync_data.prefix("app.bsky.feed.like/");
        while let Some((k, _, _)) = like_records.next()? {
            likes += 1;
        }
        self.set_likes_for_user(batch.inner_mut(), repo.did(), likes)?;
        Ok(())
    }

    /// handle account deletions etc
    fn apply_status(
        &self,
        _r: &Repo,
        _s: &Status,
        _b: &mut Self::Engine::Batch,
    ) -> AppResult<Self::Engine, ()> {
        Ok(())
    }

    /// TODO: apply_handle_change
}


/// wire it in!
#[tokio::main]
async fn main() -> Result<()> {
    // you own the database
    let mut opts = RocksOpts::default();
    opts.create_if_missing(true);
    opts.create_missing_column_families(true);
    let db = Arc::new(DB::open_cf_descriptors(
        &opts,
        "./data",
        RocksEngine::default_cf_descriptors(), // TODO (this will change)
    )?);

    // and you let hubble's storage engine use it
    let storage = RocksEngine::new(db.clone(), &Default::default())?;

    // your stuff
    let app = LikesCounter { db, points_cf: "default" };

    // aaand go-time
    HubbleSync::new(app, storage, &Default::default()).run().await?;

    Ok(())
}

```


## shared embedded storage

for now, most of the trait interface is designed around the synchronous data store apis from fjall and rocksdb.

hubble-sync assumes that apps will coexist in the same data system. this enables the consistency properties (below) and makes the backup/restore story clear and simple (rocksdb backups Just Work; fjall with filesystem-level CoW snapshots should do).

but raw key-value stores with manual secondary-indexing and no query engine can be a bit of a pain! there's no answer here for that, for now. you *can* always keep your own data in a secondary store, and give up some of the nice consistency guarantees.


## consistency

hubble-sync hands you a `WriteBatch` to stage any mutations from commits and resyncs. anything you put in the WriteBatch gets atomically committed with the sync machinery's own state transitions, so you can lean fully on the robustness of atproto sync1.1 to keep your data accurately up to date.

if you need to commit data larger than what would fit in a reasonable `WriteBatch` (for example: a Hubble resync on a very-large repo with all keys changed), it's up to you to design your app to write batched updates that can be atomically transitioned to the new state. (can be tricky but usually not that bad).


## per-repo serialization

hubble-sync uses a central actor-inspired `Repository` instance to coordinate *all* per-repo work. Events that modify repository state are routed to a repo's `Repository` instance through a queue, and processed one-by-one -- there is no concurrent mutating work performed on a repository, by construction. `Repository` instances stay alive while they have pending work, and then become eligible for unloading. The longest unload-eligible Repositories are unloaded when space is needed for new Repositories to wake up.

in addition to simplifying state transitions, this model also reduces database reads in the consumer write path (which is expensive for LSM databases), because an alive Repository actor doesn't need to read back its previous state for the sync1.1 proof from the database -- the copy it already has *must* be in sync with the database.

read-only operations don't go have to through the Repository actor. fjall and rocksdb offer `snapshot`s to ensure a consistent view within a series of reads. and that's probably touching your own app data anyway, not the internal hubble-sync state. identity resolution reads directly from the database, but messages into (and waits for) the actor if an identity is past the hard staleness threshold.

(todo: right now identity resolution is specialized to just what hubble needs: a `did -> (host, signingKey, resolvedAt)` mapping. it probably should extract a `handle` to keep along with that trio, and offer the reverse (second half of bidirectional) check on the app-driven read paths, cached... somewhere...)


## sync1.1

newly-discovered PDS hosts are considered `lax` until any `#sync` event or `#commit` with a `prevData` property present are observed (indicating a `sync-1.1`-implementing PDS), after which they are marked `strict`, and have full sync1.1 validation applied on each commit. almost all PDS hosts on the network now emit strict `sync1.1`-style events.

commits from `lax` hosts always transition repos into `desynchronized` state, because we cannot apply sync1.1's lightweight inductive proof. these repos get full `resync`s scheduled on a slow cadence: no content is actually missed, but can be significantly delayed.

todo: describe the desynchronized-latest event queue kept to manage the data race on resync (like eg., Tap has) which might become a bit trickier to deal with if people use Hubble as a getRepo fallback and get slightly-stale repos more often.


## passive mode

just need a place to stash this note for now: some apps don't want to backfill or resync (eg., a relay). they still probably want to *know* about commit gaps.

a jetstream impl based on hubble-sync might add a new emitted event type (somewhat like #sync) to notify downstream consumers of a discontinuity.


---

## thinking about

- a hook for initializing some repo state, which gets passed with the apply_* callbacks (so you can stash your own stuff in memory with the actor to avoid redundant db reads)

- right now we drop "too-old" unacked seqs from holding up the highwater mark (persisted upstream cursor) too long. this is fine for `#commit`s and `#sync`s because we will eventually detect gaps from missing events, but maybe less fine for `#account` events, which cannot be detected-missing if dropped. should the highwater mark advancing past pending seqs be event-type-dependent? (and then what do we do if an `#account` is sitting behind a `#sync` in a repo queue that's forever to process? should `Intake` be aware of the presence, and take action to drop other (reconcileable) work if certain events are being held up? at some level it should already -- incoming account event with `active=false` can clear pending work (and eagerly set desynchronized if any cleared))


## actual todos

- [x] be nice to all pdses except mushrooms (10+req/sec + high concurrency kills everyone else)
    - [x] ..triple check this
- [x] we're re-dispatching resyncs per host many times, not advancing to that host's next item
- [x] we don't have repo discovery / crawling yet
- [x] we don't have local-inactive state tracking yet (distinguishable from upstream)
- [x] the big-mem permit acquisition can wait arbitrarily long, and apparently does end up waiting a lot. for pre-request acquisition this is fine (probably? i guess we can have a lot stacked?) but when we realize it's needed mid-request, the request is likely to fail to resume after a while of no progress (30s?). the nice thing would be to give up, and set a flag so that we try to pre-acquire next time, but right now the `is_big` fn doesn't really accomodate that on its own, so we'd need to add additional state. or, we could drop the request but immediately go back to the pre-acquire path without re-entering the resync queue, where it's fine to keep waiting for a long time. in this version, we lose the useful state (this repo should be pre-permitted) on app restart. probably the persistent flag ugh.
- [x] can we limit the number of big-mem semaphore waiters? read until limit, check if too many waiters, if so just bail and reschedule for later (with big-mem hint)
    - [x] fixed via timeout on acquisition -- retry preacquires
- [ ] add DesyncReason::NotFoundUpstream

- [x] we're getting bogged down on big-mem token acquisition

- [x] for pds-upstream: need direct upstream getRepo, bc otherwise people who have migrated get requested from other PDSes get resynced from their new place (we only want to sync this pds)

- [x] exclude any non-`active` repos from the upstream crawl (though maybe it would be nice to record their account status?)
    - [ ] should check upstream for account status? eg., from crawl-discovered repos
        - this is slightly deferrable since we request from upstream now

- [x] fix public storage engine apis to not access under hubble's prefix (eek)

- [x] back off all requests to a host for 429s and 502/503/504

- [x] bsky pdses still limit 10req/sec, so drop our 15 self-throttle down

- [x] bsky pds not found is `NotFound` xrpc 400 -- double-check we catch that specifically

- [x] gauge: rss
- [x] ulimit helper
- [x] storage metrics by column family (and key range?)
- [x] figure out output compression (just do it in the reverse proxy?)

- [x] deep-crawl strategy (listHosts -> listRepos)
    - [ ] we'll want to add the upstream status checks for this probably

- [ ] listRepos periodic crawler should sync upstream account status (eventually consistent)

- [ ] set an app name for the user-agent, not just a contact info

- [ ] guard against ssrf

- [ ] sync new repos directly (no-resync) from firehose when they start from definitely-empty
- [ ] for firehose-discovered repos, we can (and should) use the car slice to predict if it's big! (from mst height)
- [ ] the firehose high water seq mark is nice but... i think we can just persist a sequence a few seconds behind for the same effect without acks-per-message flying around?
- [ ] the actual sync1.1 ratchet
    - [ ] we definitely actually want a per-host storage key actually
    - [ ] and probably hosts-as-actors at some point
- [ ] server rate-limits
- [ ] cache CARs (reverse-proxy with rev-checks reaching back to us?)
- [ ] add plc export stream; plc tombstoning
- [ ] commit: replay-vs-live flag
- [ ] `sync_handle`: using the term 'handle' in the public api is unfortunate! rename to something less confusing.
- [ ] if we're in pds-upstream mode and a resync fails and the identity says its pds is elswhere (maybe even before the fail?) then we could mark it gone from that upstream (even though the upstream doesn't know) instead of a whole retry backoff schedule
- [ ] CoW repo info for commit handling
- [ ] single-thread op inversion (watch pre-commit validation time metric)
