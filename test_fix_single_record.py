#!/usr/bin/env python3
"""
Quick test to fix a single broken record and verify it works.
"""

import json
import psycopg2
import base64

# Database config
DB_CONFIG = {
    'host': 'localhost',
    'port': 15432,
    'user': 'bsky',
    'password': 'BEVoNPm7z0lT5tMAv6hF5SQUMkIQBTRHhx0JiKjxCsdVTR274zxdPw5o9CGtpmgh',
    'database': 'bsky'
}

# Test record URI
TEST_URI = 'at://did:plc:meqmwbhzho65fuef7notuzr5/app.bsky.feed.post/3m3jxirw6rv27'


def bytes_to_cid(byte_array):
    """Convert byte array to CID string."""
    import base64

    if len(byte_array) != 36:
        raise ValueError(f"Expected 36 bytes, got {len(byte_array)}")

    version, codec, hash_fn, hash_len = byte_array[:4]

    if version != 1 or codec != 0x55 or hash_fn != 0x12 or hash_len != 32:
        raise ValueError(f"Unexpected CID format: v={version}, codec={codec}, hash={hash_fn}, len={hash_len}")

    # CIDv1: encode ALL bytes (including version) as base32, then add multibase prefix
    cid_bytes = bytes(byte_array)
    encoded = base64.b32encode(cid_bytes).decode('ascii').lower().rstrip('=')

    # 'b' = base32 multibase prefix
    return 'b' + encoded


def fix_json_cids(json_str):
    """Recursively fix all byte array CIDs in JSON."""
    data = json.loads(json_str)
    fixes_made = 0

    def fix_recursive(obj):
        nonlocal fixes_made

        if isinstance(obj, dict):
            # Check if this is a CID ref with byte array
            if 'ref' in obj and isinstance(obj['ref'], list) and len(obj['ref']) == 36:
                try:
                    cid_string = bytes_to_cid(obj['ref'])
                    obj['ref'] = {'$link': cid_string}
                    fixes_made += 1
                    print(f"    Fixed CID: {cid_string[:30]}...")
                except Exception as e:
                    print(f"    Warning: Failed to convert CID: {e}")

            # Recursively process all dict values
            return {k: fix_recursive(v) for k, v in obj.items()}

        elif isinstance(obj, list):
            return [fix_recursive(item) for item in obj]

        else:
            return obj

    fixed_data = fix_recursive(data)
    fixed_json = json.dumps(fixed_data, ensure_ascii=False)

    return fixed_json, fixes_made


def main(dry_run=True):
    print("=" * 60)
    print("Testing CID Corruption Fix on Single Record")
    print("=" * 60)
    print(f"Test URI: {TEST_URI}")
    print(f"Mode: {'DRY RUN' if dry_run else 'LIVE UPDATE'}")
    print()

    conn = psycopg2.connect(**DB_CONFIG)
    cursor = conn.cursor()

    try:
        # Fetch the broken record
        print("1. Fetching record from database...")
        cursor.execute("SELECT uri, json FROM record WHERE uri = %s", (TEST_URI,))
        result = cursor.fetchone()

        if not result:
            print(f"   ✗ Record not found!")
            return

        uri, json_str = result
        print(f"   ✓ Record found ({len(json_str)} bytes)")

        # Show original (broken) format
        print("\n2. Original JSON (first 200 chars):")
        print(f"   {json_str[:200]}...")

        # Fix the CIDs
        print("\n3. Fixing CIDs...")
        try:
            fixed_json, num_fixes = fix_json_cids(json_str)
            print(f"   ✓ Fixed {num_fixes} CID(s)")
        except Exception as e:
            print(f"   ✗ Fix failed: {e}")
            import traceback
            traceback.print_exc()
            return

        # Show fixed format
        print("\n4. Fixed JSON (first 200 chars):")
        print(f"   {fixed_json[:200]}...")

        # Update database
        if not dry_run:
            print("\n5. Updating database...")
            cursor.execute("UPDATE record SET json = %s WHERE uri = %s", (fixed_json, uri))
            conn.commit()
            print(f"   ✓ Database updated!")
        else:
            print("\n5. [DRY RUN] Would update database")

        print("\n" + "=" * 60)
        print(f"{'DRY RUN ' if dry_run else ''}SUCCESS!")
        print(f"Fixed {num_fixes} CID(s) in {uri}")
        print("=" * 60)

    finally:
        cursor.close()
        conn.close()


if __name__ == '__main__':
    import sys

    dry_run = '--live' not in sys.argv

    if not dry_run:
        response = input("Are you sure you want to UPDATE the database? (yes/no): ")
        if response.lower() != 'yes':
            print("Aborted.")
            sys.exit(0)

    main(dry_run=dry_run)
