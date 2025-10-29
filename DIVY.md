# appview backfilling

There is now a branch that is all merged with divy's backfill code: https://github.com/bluesky-social/atproto/tree/divy/backfill.  A docker image is built from the commit on the top of that branch.  I'd recommend forking from there, so that you'll have a way to add your own patches as needed.  Service entrypoints live in services/bsky/.  The interesting ones for backfill are services/bsky/ingester.js, services/bsky/backfiller.js, and services/bsky/indexer.js.

 - ingester.js: scans through `sync.listRepos` and `sync.subscribeRepos` at a given host (or hosts).  Repos are queued into redis for backfill.  Events from `subscribeRepos` are queued into redis for indexing.  May also be configured to ingest labels on `label.subscribeLabels`.
 - backfiller.js: consumes repo backfill queue from Redis.  For each repo, pulls it and queues its records in redis for indexing.  Many of these can be run in order to scale out repo processing, if needed.  They run as a consumer group against redis.
 - indexer.js: consumes the live and backfill streams from redis, indexing each operation into postgres.  Many of these can be run in order to scale out repo processing, if needed.  Will also index labels if the ingester produces them.  They run as a consumer group against redis.

A couple more have been added for the read path:
- api-basic.js: the API server of the bsky appview, all xrpc endpoint.  it uses the dataplane as its backend.
- dataplane.js: the API server for the dataplane, all grpc endpoints.  uses postgres as its backend.

You'll find a compose.yaml file attached which stands up redis, postgres, an ingester, a backfiller, and two indexers.  It now also includes an api instance and two dataplane instances.  Nothing is tuned or anything like that, but it at minimum illustrates how to wire everything up.

Worth perusing the env values in the compose file.  There are some more that can be tuned that aren't included in the compose file, see the codebase.  Before starting you do need to fill in `BSKY_SERVICE_SIGNING_KEY` in the compose file.  The signing key can be generated with:
```sh
openssl ecparam --name secp256k1 --genkey --noout --outform DER | tail --bytes=+8 | head --bytes=32 | xxd --plain --cols 32
```

A few additional notes:
 - Handles are being maintained on account events, identity events, and writes to the profile record.
 - The `IdResolver` now caches in Redis.
 - This code isn't doing sync1.1 or signature checks— it is trusting the output of the relay and labeler.  Signature checks could be added in without too much trouble.
 - You'll want to ensure redis state is being persisted to disk, as should be the case in the compose file.
 - If you need to get a repo backfilled you can always yeet it directly into the redis stream being used for backfill.
 - Take a look at the state in redis!  Four redis streams: `firehose_live`, `firehose_backfill`, `label_live`, and `repo_backfill`.  And then three cursors for `sync.listRepos`, `sync.subscribeRepos`, and `label.subscribeLabels`: `repo_backfill:cursor:morel.us-east.host.bsky.network`, `label_live:cursor:mod.bsky.app`, and `firehose_live:cursor:morel.us-east.host.bsky.network` respectively.
   ```
   root@myhost# redis-cli
   127.0.0.1:6379> keys repo*
   1) "repo_backfill"
   2) "repo_backfill:cursor:morel.us-east.host.bsky.network"
   127.0.0.1:6379> keys firehose*
   1) "firehose_backfill"
   2) "firehose_live"
   3) "firehose_live:cursor:morel.us-east.host.bsky.network"
   127.0.0.1:6379> keys label*
   1) "label_live"
   2) "label_live:cursor:mod.bsky.app"
   ```
 - You can always yeet repos you want backfilled into the `repo_backfill` redis stream— it doesn't necessarily need to come from `sync.listRepos` via the ingester.
