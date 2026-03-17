#!/usr/bin/env python3
"""
Priority backfill followers for active Blacksky appview users.

Collects follower DIDs for active users via the public Bluesky API,
deduplicates them, and outputs a CSV file for queue_backfill.

Usage:
    # Step 1: Get active DIDs from PostgreSQL on the appview server
    sudo -u postgres psql -d appview_db -t -A -c \
        "SELECT DISTINCT did FROM bsky.actor WHERE handle IS NOT NULL LIMIT 3000" \
        > /tmp/active_dids.txt

    # Step 2: Run this script
    python3 scripts/priority_backfill_followers.py \
        --input /tmp/active_dids.txt \
        --output /tmp/follower_dids.csv

    # Step 3: Queue for backfill
    queue_backfill csv --file /tmp/follower_dids.csv --immediate
"""

import argparse
import json
import os
import sys
import time
import urllib.error
import urllib.request

PUBLIC_API = "https://public.api.bsky.app"
FOLLOWERS_PER_PAGE = 100
RATE_LIMIT_DELAY = 0.125  # 8 requests/sec baseline
MAX_RETRIES = 3
CHECKPOINT_FILE = "checkpoint.json"
PROGRESS_INTERVAL = 10  # Log every N users


def load_dids(path):
    """Load DIDs from a text file (one per line)."""
    dids = []
    with open(path) as f:
        for line in f:
            did = line.strip()
            if did and did.startswith("did:"):
                dids.append(did)
    return dids


def load_checkpoint(checkpoint_path):
    """Load checkpoint state (completed DIDs and follower set)."""
    if not os.path.exists(checkpoint_path):
        return {"completed": [], "follower_dids": []}
    with open(checkpoint_path) as f:
        return json.load(f)


def save_checkpoint(checkpoint_path, completed, follower_dids_set):
    """Save checkpoint state."""
    data = {
        "completed": completed,
        "follower_dids": list(follower_dids_set),
    }
    tmp_path = checkpoint_path + ".tmp"
    with open(tmp_path, "w") as f:
        json.dump(data, f)
    os.replace(tmp_path, checkpoint_path)


def get_followers(did, delay):
    """Paginate through all followers for a DID. Returns list of follower DIDs."""
    followers = []
    cursor = None
    page = 0

    while True:
        url = f"{PUBLIC_API}/xrpc/app.bsky.graph.getFollowers?actor={did}&limit={FOLLOWERS_PER_PAGE}"
        if cursor:
            url += f"&cursor={cursor}"

        for attempt in range(MAX_RETRIES):
            try:
                time.sleep(delay)
                req = urllib.request.Request(url)
                req.add_header("Accept", "application/json")
                with urllib.request.urlopen(req, timeout=30) as resp:
                    data = json.loads(resp.read())
                break
            except urllib.error.HTTPError as e:
                if e.code == 429:
                    retry_after = int(e.headers.get("Retry-After", "30"))
                    print(f"  Rate limited, waiting {retry_after}s...")
                    time.sleep(retry_after)
                    delay = min(delay * 2, 2.0)  # Back off
                    continue
                elif e.code == 400:
                    # Actor not found / invalid
                    print(f"  Skipping {did}: HTTP {e.code}")
                    return followers, delay
                else:
                    print(f"  HTTP {e.code} for {did} (attempt {attempt + 1}/{MAX_RETRIES})")
                    if attempt == MAX_RETRIES - 1:
                        return followers, delay
                    time.sleep(2 ** attempt)
            except Exception as e:
                print(f"  Error for {did} (attempt {attempt + 1}/{MAX_RETRIES}): {e}")
                if attempt == MAX_RETRIES - 1:
                    return followers, delay
                time.sleep(2 ** attempt)

        for follower in data.get("followers", []):
            follower_did = follower.get("did")
            if follower_did:
                followers.append(follower_did)

        page += 1
        cursor = data.get("cursor")
        if not cursor or not data.get("followers"):
            break

    return followers, delay


def main():
    parser = argparse.ArgumentParser(
        description="Collect follower DIDs for active Blacksky users"
    )
    parser.add_argument(
        "--input", required=True, help="File containing active DIDs (one per line)"
    )
    parser.add_argument(
        "--output", required=True, help="Output CSV file (one DID per line)"
    )
    parser.add_argument(
        "--checkpoint",
        default=CHECKPOINT_FILE,
        help=f"Checkpoint file for resumability (default: {CHECKPOINT_FILE})",
    )
    args = parser.parse_args()

    active_dids = load_dids(args.input)
    print(f"Loaded {len(active_dids)} active DIDs")

    checkpoint = load_checkpoint(args.checkpoint)
    completed_set = set(checkpoint["completed"])
    follower_dids = set(checkpoint["follower_dids"])
    print(f"Resuming: {len(completed_set)} users done, {len(follower_dids)} unique followers collected")

    remaining = [d for d in active_dids if d not in completed_set]
    print(f"Remaining: {len(remaining)} users to process")

    delay = RATE_LIMIT_DELAY
    total_api_calls = 0
    start_time = time.time()

    for i, did in enumerate(remaining):
        followers, delay = get_followers(did, delay)
        before = len(follower_dids)
        follower_dids.update(followers)
        new_unique = len(follower_dids) - before

        completed_set.add(did)

        if (i + 1) % PROGRESS_INTERVAL == 0 or i == 0:
            elapsed = time.time() - start_time
            rate = (i + 1) / elapsed * 3600 if elapsed > 0 else 0
            print(
                f"  [{i + 1}/{len(remaining)}] {did}: "
                f"{len(followers)} followers ({new_unique} new unique) | "
                f"Total unique: {len(follower_dids)} | "
                f"{rate:.0f} users/hr"
            )

        # Checkpoint every 10 users
        if (i + 1) % PROGRESS_INTERVAL == 0:
            save_checkpoint(args.checkpoint, list(completed_set), follower_dids)

    # Final checkpoint
    save_checkpoint(args.checkpoint, list(completed_set), follower_dids)

    # Write output CSV
    with open(args.output, "w") as f:
        for did in sorted(follower_dids):
            f.write(f"{did}\n")

    elapsed = time.time() - start_time
    print(f"\nDone!")
    print(f"  Users processed: {len(completed_set)}")
    print(f"  Unique follower DIDs: {len(follower_dids)}")
    print(f"  Output: {args.output}")
    print(f"  Elapsed: {elapsed / 3600:.1f} hours")


if __name__ == "__main__":
    main()
