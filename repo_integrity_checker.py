#!/usr/bin/env python3
"""
Repo Integrity Checker - Analyze AT Protocol repo state vs indexed state

This tool:
1. Fetches a repo CAR file from a PDS
2. Parses all records and their timestamps
3. Queries PostgreSQL to see what's indexed
4. Identifies missing records
5. Searches Redis streams for missing records
6. Estimates processing time

Usage:
    python3 repo_integrity_checker.py did:plc:w4xbfzo7kqfes5zb7r6qv3rw
"""

import sys
import requests
import json
import psycopg2
import redis
from datetime import datetime, timezone
from typing import Dict, List, Set, Tuple
from collections import defaultdict

# Configuration
PDS_HOST = "blacksky.app"
REDIS_HOST = "localhost"
REDIS_PORT = 6380
PG_HOST = "localhost"
PG_PORT = 15433
PG_USER = "bsky"
PG_PASS = "BEVoNPm7z0lT5tMAv6hF5SQUMkIQBTRHhx0JiKjxCsdVTR274zxdPw5o9CGtpmgh"
PG_DB = "bsky"

def fetch_repo_car(did: str) -> bytes:
    """Fetch repo CAR file from PDS"""
    url = f"https://{PDS_HOST}/xrpc/com.atproto.sync.getRepo?did={did}"
    print(f"Fetching repo CAR from {url}...")

    response = requests.get(url, timeout=60)
    response.raise_for_status()

    print(f"Downloaded {len(response.content)} bytes")
    return response.content

def parse_car_file(car_data: bytes) -> Dict[str, any]:
    """
    Parse CAR file to extract records

    For now, we'll use a simplified approach since full CAR parsing requires
    multiformats and CBOR libraries. We can enhance this later.
    """
    # TODO: Implement full CAR parsing
    # For now, return empty dict and we'll fetch via XRPC instead
    print("NOTE: Full CAR parsing not yet implemented")
    print("      Will fetch records via XRPC listRecords instead")
    return {}

def fetch_records_via_xrpc(did: str) -> Dict[str, List[Dict]]:
    """Fetch all records for a DID using listRecords XRPC method"""
    collections = [
        "app.bsky.feed.post",
        "app.bsky.feed.like",
        "app.bsky.feed.repost",
        "app.bsky.feed.threadgate",
        "app.bsky.graph.follow",
        "app.bsky.graph.block",
    ]

    all_records = defaultdict(list)

    for collection in collections:
        cursor = None
        while True:
            url = f"https://{PDS_HOST}/xrpc/com.atproto.repo.listRecords"
            params = {
                "repo": did,
                "collection": collection,
                "limit": 100
            }
            if cursor:
                params["cursor"] = cursor

            try:
                response = requests.get(url, params=params, timeout=30)
                response.raise_for_status()
                data = response.json()

                records = data.get("records", [])
                all_records[collection].extend(records)

                cursor = data.get("cursor")
                if not cursor or not records:
                    break

                print(f"  {collection}: {len(all_records[collection])} records fetched...")
            except Exception as e:
                print(f"  Error fetching {collection}: {e}")
                break

    return all_records

def query_indexed_records(did: str) -> Dict[str, Set[str]]:
    """Query PostgreSQL for indexed records"""
    conn = psycopg2.connect(
        host=PG_HOST,
        port=PG_PORT,
        user=PG_USER,
        password=PG_PASS,
        database=PG_DB
    )

    cur = conn.cursor()
    cur.execute("""
        SELECT uri, json, "indexedAt"
        FROM record
        WHERE did = %s
    """, (did,))

    indexed = defaultdict(set)
    for uri, json_data, indexed_at in cur:
        # Extract collection from URI: at://did/collection/rkey
        parts = uri.split('/')
        if len(parts) >= 5:
            collection = parts[3]
            rkey = parts[4]
            indexed[collection].add(rkey)

    cur.close()
    conn.close()

    return indexed

def search_redis_for_did(did: str) -> Dict[str, List[str]]:
    """Search Redis streams for messages containing this DID"""
    r = redis.Redis(host=REDIS_HOST, port=REDIS_PORT, decode_responses=True)

    streams = ["firehose_live", "firehose_backfill", "repo_backfill"]
    results = {}

    for stream in streams:
        try:
            # Get stream length
            length = r.xlen(stream)
            print(f"\nSearching {stream} ({length:,} messages)...")

            # Sample recent messages (last 1000)
            messages = r.xrevrange(stream, count=1000)

            found = []
            for msg_id, data in messages:
                # Check if DID is in message data
                for key, value in data.items():
                    if did in str(value):
                        found.append(msg_id)
                        break

            results[stream] = found
            if found:
                print(f"  Found {len(found)} messages containing {did}")
        except Exception as e:
            print(f"  Error searching {stream}: {e}")
            results[stream] = []

    return results

def verify_missing_records_in_redis(did: str, missing: Dict[str, List[Dict]]) -> Dict[str, tuple]:
    """
    Verify each missing record is present in Redis streams.

    Returns dict of rkey -> (stream_name, message_id) for found records.
    """
    r = redis.Redis(host=REDIS_HOST, port=REDIS_PORT, decode_responses=True)

    # Extract all rkeys from missing records
    all_rkeys = set()
    rkey_to_collection = {}

    for collection, records in missing.items():
        for rec in records:
            rkey = rec.get("uri", "").split("/")[-1]
            if rkey:
                all_rkeys.add(rkey)
                rkey_to_collection[rkey] = collection

    print(f"\n=== Verifying Each Missing Record in Redis ===")
    print(f"Searching for {len(all_rkeys)} specific rkeys...")

    streams = ["firehose_live", "firehose_backfill"]
    all_found = {}
    batch_size = 10000
    max_batches = 20  # Check up to 200K messages per stream

    for stream in streams:
        stream_len = r.xlen(stream)
        print(f"\n🔍 Searching {stream} ({stream_len:,} messages)...")

        cursor = '+'  # Start from newest
        messages_scanned = 0

        for batch in range(max_batches):
            try:
                messages = r.xrevrange(stream, max=cursor, count=batch_size)
                if not messages:
                    break

                messages_scanned += len(messages)

                for msg_id, data in messages:
                    # Check if this message is for our DID
                    msg_did = data.get('did', '')
                    if msg_did != did:
                        continue

                    # Extract rkey from path: /app.bsky.feed.post/3m4b7h7auis2e
                    path = data.get('path', '')
                    if '/' in path:
                        msg_rkey = path.split('/')[-1]
                        if msg_rkey in all_rkeys and msg_rkey not in all_found:
                            all_found[msg_rkey] = (stream, msg_id)
                            collection = rkey_to_collection.get(msg_rkey, "unknown")
                            print(f"  ✅ Found {msg_rkey} ({collection}) at {msg_id}")

                # Update cursor for next batch
                cursor = messages[-1][0]

                if batch % 5 == 4:
                    print(f"  ... scanned {messages_scanned:,} messages, found {len(all_found)}/{len(all_rkeys)} rkeys")

            except Exception as e:
                print(f"  Error in batch {batch}: {e}")
                break

        print(f"  Total scanned: {messages_scanned:,} messages in {stream}")

    # Report results
    print(f"\n=== Verification Results ===")
    print(f"Total missing records: {len(all_rkeys)}")
    print(f"Found in Redis: {len(all_found)}")
    print(f"NOT found: {len(all_rkeys) - len(all_found)}")

    if len(all_found) < len(all_rkeys):
        print(f"\n⚠️  WARNING: {len(all_rkeys) - len(all_found)} records NOT found in Redis streams!")
        print(f"\nMissing rkeys not found in Redis:")
        not_found = all_rkeys - set(all_found.keys())
        for rkey in sorted(not_found):
            collection = rkey_to_collection.get(rkey, "unknown")
            print(f"  - {rkey} ({collection})")
    else:
        print(f"\n✅ ALL {len(all_rkeys)} missing records confirmed in Redis queues!")

    # Show distribution
    stream_counts = defaultdict(int)
    for rkey, (stream, msg_id) in all_found.items():
        stream_counts[stream] += 1

    print(f"\nDistribution by stream:")
    for stream, count in stream_counts.items():
        print(f"  {stream}: {count}")

    return all_found

def analyze_date_range(records: Dict[str, List[Dict]], start_date: str, end_date: str) -> Dict:
    """Filter records to a specific date range"""
    start = datetime.fromisoformat(start_date.replace('Z', '+00:00'))
    end = datetime.fromisoformat(end_date.replace('Z', '+00:00'))

    filtered = defaultdict(list)
    for collection, recs in records.items():
        for rec in recs:
            created_at_str = rec.get("value", {}).get("createdAt")
            if created_at_str:
                try:
                    created_at = datetime.fromisoformat(created_at_str.replace('Z', '+00:00'))
                    if start <= created_at <= end:
                        filtered[collection].append(rec)
                except:
                    pass

    return filtered

def main():
    if len(sys.argv) < 2:
        print("Usage: python3 repo_integrity_checker.py <did> [start_date] [end_date]")
        print("Example: python3 repo_integrity_checker.py did:plc:w4xbfzo7kqfes5zb7r6qv3rw")
        print("         python3 repo_integrity_checker.py did:plc:w4xbfzo7kqfes5zb7r6qv3rw 2025-10-14 2025-10-31")
        sys.exit(1)

    did = sys.argv[1]
    start_date = sys.argv[2] if len(sys.argv) > 2 else None
    end_date = sys.argv[3] if len(sys.argv) > 3 else None

    print(f"=== Repo Integrity Check for {did} ===")
    if start_date and end_date:
        print(f"Date Range: {start_date} to {end_date}\n")
    else:
        print()

    # Step 1: Fetch records from PDS
    print("Step 1: Fetching records from PDS...")
    pds_records = fetch_records_via_xrpc(did)

    total_pds = sum(len(recs) for recs in pds_records.values())
    print(f"\nTotal records from PDS: {total_pds}")
    for collection, recs in pds_records.items():
        print(f"  {collection}: {len(recs)}")

    # Step 2: Query indexed records
    print("\nStep 2: Querying indexed records from PostgreSQL...")
    indexed_records = query_indexed_records(did)

    total_indexed = sum(len(rkeys) for rkeys in indexed_records.values())
    print(f"\nTotal indexed records: {total_indexed}")
    for collection, rkeys in indexed_records.items():
        print(f"  {collection}: {len(rkeys)}")

    # Step 3: Find missing records
    print("\nStep 3: Comparing PDS vs PostgreSQL...")
    missing = defaultdict(list)

    for collection, recs in pds_records.items():
        indexed_rkeys = indexed_records.get(collection, set())
        for rec in recs:
            rkey = rec.get("uri", "").split("/")[-1]
            if rkey and rkey not in indexed_rkeys:
                missing[collection].append(rec)

    # Filter missing records by date range if specified
    if start_date and end_date:
        start_dt = datetime.fromisoformat(start_date + 'T00:00:00Z')
        end_dt = datetime.fromisoformat(end_date + 'T23:59:59Z')

        missing_filtered = defaultdict(list)
        for collection, recs in missing.items():
            for rec in recs:
                created_at_str = rec.get("value", {}).get("createdAt")
                if created_at_str:
                    try:
                        created_at = datetime.fromisoformat(created_at_str.replace('Z', '+00:00'))
                        if start_dt <= created_at <= end_dt:
                            missing_filtered[collection].append(rec)
                    except:
                        pass

        print(f"\n❌ Missing records (filtered to {start_date} - {end_date}): {sum(len(recs) for recs in missing_filtered.values())}")
        missing = missing_filtered
    else:
        print(f"\n❌ Missing records (all time): {sum(len(recs) for recs in missing.values())}")

    total_missing = sum(len(recs) for recs in missing.values())
    for collection, recs in missing.items():
        print(f"  {collection}: {len(recs)}")
        if recs and len(recs) <= 20:
            for rec in recs[:10]:
                created_at = rec.get("value", {}).get("createdAt", "unknown")
                rkey = rec.get("uri", "").split("/")[-1]
                print(f"    - {rkey} (created: {created_at})")

    # Step 4: Verify each missing record in Redis
    if total_missing > 0:
        print(f"\nStep 4: Verifying all {total_missing} missing records in Redis streams...")
        found_in_redis = verify_missing_records_in_redis(did, missing)

        # Calculate data loss
        not_found_count = total_missing - len(found_in_redis)
        if not_found_count > 0:
            print(f"\n⚠️  CRITICAL: {not_found_count} records may be permanently lost!")
        else:
            print(f"\n✅ SUCCESS: All {total_missing} missing records confirmed in Redis queues!")

    print("\n=== Analysis Complete ===")

if __name__ == "__main__":
    main()
