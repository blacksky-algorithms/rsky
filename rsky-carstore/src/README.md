# rsky-carstore

Store a zillion users of PDS-like repo, with more limited operations (mainly: firehose in, firehose out).

## [SurrealCarstore](carstore.rs)

Store 'car slices' from PDS source subscribeRepo firehose streams and metadata to filesystem via SurrealDB.
Uses SurrealDB in an embedded context backed by RocksDB.
Periodic compaction of car slices into fewer larger car slices.
Based on Bluesky's FileCarStore which was the first production carstore and used through at least 2024-11.
