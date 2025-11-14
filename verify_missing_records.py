#!/usr/bin/env python3
"""
Verify Missing Records in Redis Streams

Searches Redis streams for specific missing records to confirm they're
queued and not lost.
"""

import redis
import json
import sys
from collections import defaultdict

REDIS_HOST = "localhost"
REDIS_PORT = 6380

def search_stream_for_rkeys(stream_name: str, rkeys: list[str], did: str, batch_size: int = 10000):
    """
    Search a Redis stream for specific record keys (rkeys).

    Returns dict of found rkeys with their message IDs.
    """
    r = redis.Redis(host=REDIS_HOST, port=REDIS_PORT, decode_responses=True)

    stream_len = r.xlen(stream_name)
    print(f"\n🔍 Searching {stream_name} ({stream_len:,} messages)...")
    print(f"   Looking for {len(rkeys)} missing rkeys...")

    found = {}
    messages_scanned = 0

    # Search in chunks from newest to oldest
    # Strategy: Check recent messages first (most likely location)
    cursor = '+'  # Start from newest
    chunks_to_check = min(20, stream_len // batch_size + 1)  # Check up to 200K messages

    for chunk in range(chunks_to_check):
        messages = r.xrevrange(stream_name, max=cursor, count=batch_size)
        if not messages:
            break

        messages_scanned += len(messages)

        for msg_id, data in messages:
            # Check if this message is for our DID
            msg_did = data.get('did', '')
            if msg_did != did:
                continue

            # Extract rkey from the message
            # Messages contain 'path' like: /app.bsky.feed.post/3m4b7h7auis2e
            path = data.get('path', '')
            if '/' in path:
                msg_rkey = path.split('/')[-1]
                if msg_rkey in rkeys:
                    found[msg_rkey] = msg_id
                    print(f"   ✅ Found {msg_rkey} at message {msg_id}")

        # Update cursor for next batch
        cursor = messages[-1][0]  # Last message ID in batch

        if chunk % 5 == 0 and chunk > 0:
            print(f"   Scanned {messages_scanned:,} messages so far... (found {len(found)} rkeys)")

    print(f"   Total scanned: {messages_scanned:,} messages")
    print(f"   Found: {len(found)} / {len(rkeys)} rkeys")

    return found

def verify_all_missing(missing_records: dict):
    """
    Verify all missing records are in Redis streams.

    missing_records: dict of collection -> list of record dicts
    """
    did = "did:plc:w4xbfzo7kqfes5zb7r6qv3rw"

    # Extract all rkeys from missing records
    all_rkeys = set()
    rkey_to_collection = {}

    for collection, records in missing_records.items():
        for rec in records:
            rkey = rec.get("uri", "").split("/")[-1]
            if rkey:
                all_rkeys.add(rkey)
                rkey_to_collection[rkey] = collection

    print(f"=== Verifying {len(all_rkeys)} Missing Records in Redis Streams ===")
    print(f"DID: {did}")
    print(f"\nMissing records by collection:")
    for collection, records in missing_records.items():
        print(f"  {collection}: {len(records)}")

    # Search both streams
    streams = ["firehose_live", "firehose_backfill"]
    all_found = {}

    for stream in streams:
        found = search_stream_for_rkeys(stream, list(all_rkeys), did)
        for rkey, msg_id in found.items():
            if rkey not in all_found:
                all_found[rkey] = (stream, msg_id)

    # Report results
    print(f"\n=== Verification Results ===")
    print(f"Total missing rkeys: {len(all_rkeys)}")
    print(f"Found in Redis: {len(all_found)}")
    print(f"Still missing: {len(all_rkeys) - len(all_found)}")

    if len(all_found) < len(all_rkeys):
        print(f"\n⚠️  WARNING: {len(all_rkeys) - len(all_found)} records NOT found in Redis!")
        print(f"\nMissing rkeys not found:")
        for rkey in all_rkeys:
            if rkey not in all_found:
                collection = rkey_to_collection.get(rkey, "unknown")
                print(f"  - {rkey} ({collection})")
    else:
        print(f"\n✅ ALL {len(all_rkeys)} missing records confirmed in Redis queues!")

    # Show distribution
    print(f"\nDistribution by stream:")
    stream_counts = defaultdict(int)
    for rkey, (stream, msg_id) in all_found.items():
        stream_counts[stream] += 1

    for stream, count in stream_counts.items():
        print(f"  {stream}: {count}")

    return all_found, all_rkeys - set(all_found.keys())

if __name__ == "__main__":
    # For now, we'll need to pass in the missing records
    # This will be integrated with repo_integrity_checker.py

    print("This script is designed to be called from repo_integrity_checker.py")
    print("Run: python3 repo_integrity_checker.py did:plc:w4xbfzo7kqfes5zb7r6qv3rw 2025-10-14 2025-10-31")
    print("Then we'll extract the missing records and verify them here.")
