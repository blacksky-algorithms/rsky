#!/usr/bin/env python3
"""
CID Corruption Migration Script

Fixes 25.2M records with byte array CIDs by converting them to proper CID strings.

Usage:
    python3 fix_cid_corruption.py --dry-run          # Preview changes
    python3 fix_cid_corruption.py --limit 100        # Fix first 100 records
    python3 fix_cid_corruption.py --batch-size 10000 # Fix all in batches of 10K
"""

import json
import psycopg2
import argparse
from typing import List, Dict, Any, Tuple
import sys

try:
    from cid import make_cid
    from multiformats import CID
    HAS_CID_LIB = True
except ImportError:
    HAS_CID_LIB = False
    print("Warning: CID library not found. Install with: pip3 install --user py-cid multiformats", file=sys.stderr)

# Database connection parameters
DB_CONFIG = {
    'host': 'localhost',
    'port': 15432,
    'user': 'bsky',
    'password': 'BEVoNPm7z0lT5tMAv6hF5SQUMkIQBTRHhx0JiKjxCsdVTR274zxdPw5o9CGtpmgh',
    'database': 'bsky'
}


def bytes_to_cid(byte_array: List[int]) -> str:
    """
    Convert a byte array CID to its string representation.

    CID format (CIDv1):
    - byte[0]: version (1)
    - byte[1]: multicodec (0x55 = raw)
    - byte[2]: multihash type (0x12 = sha256)
    - byte[3]: hash length (0x20 = 32)
    - byte[4:]: hash bytes

    Args:
        byte_array: List of integers representing the CID bytes

    Returns:
        Base32-encoded CID string (e.g., "bafkrei...")
    """
    # Try using CID library if available
    if HAS_CID_LIB:
        try:
            cid_bytes = bytes(byte_array)
            cid_obj = CID.decode(cid_bytes)
            return str(cid_obj)
        except Exception as e:
            # Fall back to manual implementation
            pass

    # Manual implementation using standard library
    import base64

    # Validate input
    if not byte_array or len(byte_array) < 4:
        raise ValueError(f"Invalid CID byte array: too short ({len(byte_array)} bytes)")

    version = byte_array[0]
    codec = byte_array[1]
    hash_fn = byte_array[2]
    hash_len = byte_array[3]

    # Validate CID structure
    if version != 1:
        raise ValueError(f"Unsupported CID version: {version}")

    if codec != 0x55:  # raw codec
        raise ValueError(f"Unsupported codec: {codec}")

    if hash_fn != 0x12:  # sha256
        raise ValueError(f"Unsupported hash function: {hash_fn}")

    if hash_len != 32:
        raise ValueError(f"Invalid hash length: {hash_len}")

    if len(byte_array) != hash_len + 4:
        raise ValueError(f"Byte array length mismatch: expected {hash_len + 4}, got {len(byte_array)}")

    # Convert to bytes and encode
    cid_bytes = bytes(byte_array)

    # For CIDv1, encode ALL bytes (including version) as base32, then add multibase prefix
    # CIDv1 uses base32 (RFC 4648) lowercase without padding, with 'b' multibase prefix
    encoded = base64.b32encode(cid_bytes).decode('ascii').lower().rstrip('=')

    # Add multibase prefix 'b' for base32
    cid_string = 'b' + encoded

    return cid_string


def fix_json_cids(json_str: str) -> Tuple[str, int]:
    """
    Recursively find and fix all byte array CIDs in a JSON string.

    Args:
        json_str: JSON string with potential byte array CIDs

    Returns:
        Tuple of (fixed_json_string, num_fixes_made)
    """
    try:
        data = json.loads(json_str)
    except json.JSONDecodeError as e:
        raise ValueError(f"Invalid JSON: {e}")

    fixes_made = 0

    def fix_recursive(obj: Any) -> Any:
        nonlocal fixes_made

        if isinstance(obj, dict):
            # Check if this is a CID ref object with byte array
            if 'ref' in obj and isinstance(obj['ref'], list):
                # This looks like a corrupted CID
                try:
                    cid_string = bytes_to_cid(obj['ref'])
                    obj['ref'] = {'$link': cid_string}
                    fixes_made += 1
                except Exception as e:
                    # Log error but continue
                    print(f"Warning: Failed to convert CID: {e}", file=sys.stderr)

            # Recursively process all dict values
            return {k: fix_recursive(v) for k, v in obj.items()}

        elif isinstance(obj, list):
            # Recursively process all list items
            return [fix_recursive(item) for item in obj]

        else:
            # Primitive types, return as-is
            return obj

    fixed_data = fix_recursive(data)
    fixed_json = json.dumps(fixed_data, ensure_ascii=False)

    return fixed_json, fixes_made


def get_broken_records_batch(cursor, last_uri: str, limit: int) -> List[Tuple[str, str]]:
    """
    Fetch a batch of broken records using cursor-based pagination.

    Args:
        cursor: Database cursor
        last_uri: Last URI processed (for cursor pagination)
        limit: Number of records to fetch

    Returns:
        List of (uri, json) tuples
    """
    if last_uri:
        query = """
            SELECT uri, json
            FROM record
            WHERE json LIKE '%%"ref":[%%'
              AND uri > %s
            ORDER BY uri
            LIMIT %s
        """
        cursor.execute(query, (last_uri, limit))
    else:
        query = """
            SELECT uri, json
            FROM record
            WHERE json LIKE '%%"ref":[%%'
            ORDER BY uri
            LIMIT %s
        """
        cursor.execute(query, (limit,))
    return cursor.fetchall()


def count_broken_records(cursor) -> int:
    """Count total broken records."""
    query = """
        SELECT COUNT(*)
        FROM record
        WHERE json LIKE '%%"ref":[%%'
    """
    cursor.execute(query)
    return cursor.fetchone()[0]


def update_record(cursor, uri: str, fixed_json: str) -> None:
    """Update a single record with fixed JSON."""
    query = """
        UPDATE record
        SET json = %s
        WHERE uri = %s
    """
    cursor.execute(query, (fixed_json, uri))


def migrate(dry_run: bool = True, limit: int = None, batch_size: int = 1000, skip_count: bool = False) -> None:
    """
    Execute the migration.

    Args:
        dry_run: If True, preview changes without applying
        limit: Maximum number of records to fix (None = all)
        batch_size: Number of records to process per batch
        skip_count: If True, skip counting total records (faster startup)
    """
    conn = psycopg2.connect(**DB_CONFIG)
    cursor = conn.cursor()

    try:
        # Count total broken records (skip if requested for faster startup)
        if skip_count:
            print("Skipping count (will process until no more records found)")
            total_to_fix = limit if limit else float('inf')
        else:
            total_broken = count_broken_records(cursor)
            print(f"Found {total_broken:,} broken records")

            if limit:
                total_to_fix = min(limit, total_broken)
                print(f"Limiting to {total_to_fix:,} records")
            else:
                total_to_fix = total_broken

        # Process in batches using cursor-based pagination
        last_uri = None
        total_processed = 0
        total_fixed = 0
        total_cids_fixed = 0

        while True:
            print(f"\nProcessing batch: last_uri={last_uri if last_uri else '(start)'}")

            # Fetch batch
            records = get_broken_records_batch(cursor, last_uri, batch_size)

            if not records:
                print("No more records found")
                break

            batch_fixed = 0
            batch_cids_fixed = 0

            for uri, json_str in records:
                try:
                    # Fix the JSON
                    fixed_json, num_fixes = fix_json_cids(json_str)

                    if num_fixes > 0:
                        if dry_run:
                            print(f"  [DRY RUN] Would fix {num_fixes} CID(s) in {uri}")
                        else:
                            update_record(cursor, uri, fixed_json)
                            print(f"  ✓ Fixed {num_fixes} CID(s) in {uri}")

                        batch_fixed += 1
                        batch_cids_fixed += num_fixes

                    # Update cursor
                    last_uri = uri

                except Exception as e:
                    print(f"  ✗ Error processing {uri}: {e}", file=sys.stderr)
                    # Still update cursor to continue
                    last_uri = uri

            if not dry_run:
                conn.commit()
                print(f"  Committed {batch_fixed} records")

            total_processed += len(records)
            total_fixed += batch_fixed
            total_cids_fixed += batch_cids_fixed

            # Progress update
            if total_to_fix == float('inf'):
                print(f"Progress: {total_processed:,} records processed")
            else:
                progress_pct = (total_processed / total_to_fix) * 100
                print(f"Progress: {total_processed:,}/{total_to_fix:,} ({progress_pct:.1f}%)")
            print(f"Total fixed so far: {total_fixed:,} records, {total_cids_fixed:,} CIDs")

            # Check if we've hit limit
            if limit and total_processed >= limit:
                print(f"Reached limit of {limit:,} records")
                break

        print(f"\n{'DRY RUN ' if dry_run else ''}COMPLETE!")
        print(f"Total records fixed: {total_fixed:,}")
        print(f"Total CIDs fixed: {total_cids_fixed:,}")

    finally:
        cursor.close()
        conn.close()


def test_cid_conversion():
    """Test CID conversion with a known example."""
    # Example from our data: [1,85,18,32,53,200,170,252,248,164,102,188,130,25,215,52,203,146,215,60,77,125,126,70,180,46,207,17,225,206,211,81,108,209,83,250]
    test_bytes = [1, 85, 18, 32, 53, 200, 170, 252, 248, 164, 102, 188, 130, 25, 215, 52, 203, 146, 215, 60, 77, 125, 126, 70, 180, 46, 207, 17, 225, 206, 211, 81, 108, 209, 83, 250]

    try:
        cid = bytes_to_cid(test_bytes)
        print(f"Test CID conversion:")
        print(f"  Input:  {test_bytes[:8]}...")
        print(f"  Output: {cid}")
        print(f"  ✓ Conversion successful")
        return True
    except Exception as e:
        print(f"  ✗ Conversion failed: {e}")
        return False


def main():
    parser = argparse.ArgumentParser(description='Fix CID corruption in Bluesky database')
    parser.add_argument('--dry-run', action='store_true', help='Preview changes without applying')
    parser.add_argument('--limit', type=int, help='Maximum records to fix')
    parser.add_argument('--batch-size', type=int, default=1000, help='Batch size for processing')
    parser.add_argument('--skip-count', action='store_true', help='Skip counting total records (faster startup)')
    parser.add_argument('--test', action='store_true', help='Run CID conversion test')
    parser.add_argument('--yes', action='store_true', help='Skip confirmation prompt')

    args = parser.parse_args()

    if args.test:
        test_cid_conversion()
        return

    print("CID Corruption Migration Script")
    print("=" * 50)

    if args.dry_run:
        print("MODE: DRY RUN (no changes will be made)")
    else:
        print("MODE: LIVE (changes will be applied)")
        if not args.yes:
            response = input("Are you sure you want to proceed? (yes/no): ")
            if response.lower() != 'yes':
                print("Aborted.")
                return

    migrate(dry_run=args.dry_run, limit=args.limit, batch_size=args.batch_size, skip_count=args.skip_count)


if __name__ == '__main__':
    main()
