import json
import sqlite3
import time

import requests

# Configuration
BASE_URL = "https://plc.directory/export"
COUNT_PER_REQUEST = 1000
SLEEP_SECONDS = 0.5
DB_FILE = "plc_directory.db"


def create_database():
    """Create SQLite database and table if they don't exist."""
    conn = sqlite3.connect(DB_FILE)
    conn.execute("""PRAGMA auto_vacuum = INCREMENTAL""")
    conn.execute("""PRAGMA journal_mode = WAL""")
    cursor = conn.cursor()

    # Set PRAGMAs
    conn.execute("""PRAGMA cache_size = -64000""")
    conn.execute("""PRAGMA journal_size_limit = 6144000""")
    conn.execute("""PRAGMA mmap_size = 268435456""")
    conn.execute("""PRAGMA secure_delete = OFF""")
    conn.execute("""PRAGMA synchronous = NORMAL""")
    conn.execute("""PRAGMA temp_store = MEMORY""")

    # Create table for PLC operations
    cursor.execute("""
    CREATE TABLE IF NOT EXISTS plc_operations (
        cid TEXT NOT NULL PRIMARY KEY ON CONFLICT REPLACE,
        did TEXT NOT NULL,
        created_at TEXT NOT NULL,
        nullified BOOLEAN NOT NULL,
        operation BLOB NOT NULL,
        pds_endpoint TEXT GENERATED ALWAYS AS (
            json_extract(operation, '$.services.atproto_pds.endpoint')
        ) STORED,
        atproto_key TEXT GENERATED ALWAYS AS (
            json_extract(operation, '$.verificationMethods.atproto')
        ) STORED,
        labeler_endpoint TEXT GENERATED ALWAYS AS (
            json_extract(operation, '$.services.atproto_labeler.endpoint')
        ) STORED,
        atproto_label_key TEXT GENERATED ALWAYS AS (
            json_extract(operation, '$.verificationMethods.atproto_label')
        ) STORED
    )
    """)

    # Drop all views
    cursor.execute("""DROP VIEW IF EXISTS plc_labelers""")
    cursor.execute("""DROP VIEW IF EXISTS plc_pdses""")
    cursor.execute("""DROP VIEW IF EXISTS plc_keys""")
    cursor.execute("""DROP VIEW IF EXISTS plc_latest""")

    # Create indexes
    cursor.execute("""
    CREATE INDEX IF NOT EXISTS idx_plc_operations_did_created_at
        ON plc_operations (did, created_at DESC)
    """)
    cursor.execute("""
    CREATE INDEX IF NOT EXISTS idx_plc_operations_pds_endpoint
        ON plc_operations (pds_endpoint, created_at)
        WHERE pds_endpoint IS NOT NULL
    """)
    cursor.execute("""
    CREATE INDEX IF NOT EXISTS idx_plc_operations_labeler_endpoint
        ON plc_operations (labeler_endpoint, created_at)
        WHERE labeler_endpoint IS NOT NULL
    """)

    # Create views
    cursor.execute("""
    CREATE VIEW plc_latest AS
        SELECT *
        FROM plc_operations
        WHERE created_at = (
            SELECT MAX(created_at)
            FROM plc_operations AS sub
            WHERE sub.did = plc_operations.did
        )
    """)
    cursor.execute("""
    CREATE VIEW plc_keys AS
        SELECT
            did,
            created_at,
            pds_endpoint,
            atproto_key AS pds_key,
            labeler_endpoint,
            atproto_label_key AS labeler_key
        FROM plc_latest
    """)
    cursor.execute("""
    CREATE VIEW plc_pdses AS
        SELECT
            MIN(created_at) AS first,
            MAX(created_at) AS last,
            count() AS accounts,
            pds_endpoint
        FROM plc_latest
        WHERE pds_endpoint IS NOT NULL
        GROUP BY pds_endpoint
        ORDER BY last
    """)
    cursor.execute("""
    CREATE VIEW plc_labelers AS
        SELECT
            did,
            created_at,
            labeler_endpoint
        FROM plc_latest
        WHERE labeler_endpoint IS NOT NULL
        ORDER BY created_at
    """)

    conn.commit()

    # Vacuum & optimize
    cursor.execute("""PRAGMA incremental_vacuum""")
    cursor.execute("""PRAGMA optimize = 0x10002""")

    return conn


def fetch_plc_operations(session, after=None):
    """Fetch PLC operations from the API using a persistent session."""
    params = {"count": COUNT_PER_REQUEST}
    if after:
        params["after"] = after

    response = session.get(BASE_URL, params=params)

    if response.status_code == 200:
        # The response is in JSON Lines format, so we need to parse each line
        return [json.loads(line) for line in response.text.split("\n")]
    else:
        print(f"Error fetching data: {response.status_code} - {response.text}")
        return []


def insert_operations(conn, operations):
    """Insert operations into the database."""
    cursor = conn.cursor()

    for op in operations:
        cursor.execute(
            """
            INSERT INTO plc_operations (cid, did, created_at, nullified, operation)
            VALUES (?, ?, ?, ?, ?)
            """,
            (
                op.get("cid"),
                op.get("did"),
                op.get("createdAt"),
                op.get("nullified"),
                json.dumps(op.get("operation"), separators=(",", ":")).encode("utf-8"),
            ),
        )

    conn.commit()
    return cursor.rowcount


def get_count(conn):
    """Get the count from the database."""
    cursor = conn.cursor()
    cursor.execute("SELECT count(*) FROM plc_operations")
    result = cursor.fetchone()
    return result[0] if result else None


def get_latest_timestamp(conn):
    """Get the latest timestamp from the database."""
    cursor = conn.cursor()
    cursor.execute(
        "SELECT created_at FROM plc_operations ORDER BY created_at DESC LIMIT 1"
    )
    result = cursor.fetchone()
    return result[0] if result else None


def main():
    conn = create_database()

    # Create a persistent session
    session = requests.Session()

    # Set common headers if needed
    session.headers.update({"User-Agent": "rsky-relay", "Accept": "application/json"})

    # Check if we have existing data and get the latest timestamp
    latest_timestamp = get_latest_timestamp(conn)
    after = latest_timestamp

    total_processed = get_count(conn) or 0
    request_count = total_processed // 999

    try:
        print("Starting PLC Directory API crawl...")

        while True:
            operations = fetch_plc_operations(session, after)
            request_count += 1

            if not operations:
                print("No more operations to fetch or API error occurred.")
                break

            insert_operations(conn, operations)
            prev_processed = total_processed
            total_processed = get_count(conn) or 0

            # Get the last timestamp for the next request
            last_op = operations[-1]
            after = last_op.get("createdAt")

            # Progress reporting
            print(
                f"Request #{request_count}: Fetched {len(operations)}, "
                f"Total {total_processed}, Last timestamp: {after}"
            )
            ignored = len(operations) - (total_processed - prev_processed)
            if ignored != 1:
                print(f"IGNORED: {ignored}")

            # Check if we got fewer records than requested (end of data)
            if len(operations) < COUNT_PER_REQUEST:
                print("Reached the end of available data.")
                break

            # Sleep to avoid overloading the server
            time.sleep(SLEEP_SECONDS)

    except KeyboardInterrupt:
        print("\nCrawl interrupted by user. Progress saved.")
    except Exception as e:
        print(f"Error occurred: {e}")
    finally:
        # Final stats
        print("\nCrawl complete or interrupted.")
        print(f"Total records processed: {total_processed}")
        print(f"Total API requests made: {request_count}")

        # Close connections
        conn.close()
        session.close()  # Close the session when done


if __name__ == "__main__":
    main()
