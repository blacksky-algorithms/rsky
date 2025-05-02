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
    cursor = conn.cursor()

    # Create table for PLC operations
    cursor.execute("""
    CREATE TABLE IF NOT EXISTS plc_operations (
        did TEXT,
        created_at TEXT,
        nullified BOOLEAN,
        cid TEXT,
        operation TEXT
    )
    """)
    cursor.execute("""
    CREATE INDEX IF NOT EXISTS did_index ON plc_operations (
	    did
    )
    """)
    cursor.execute("""
    CREATE INDEX IF NOT EXISTS created_at_index ON plc_operations (
	    created_at ASC
    )
    """)
    cursor.execute("""
    CREATE VIEW IF NOT EXISTS plc_latest AS
        SELECT *
        FROM plc_operations
        WHERE created_at = (
            SELECT MAX(created_at)
            FROM plc_operations AS sub
            WHERE sub.did = plc_operations.did
        )
    """)
    cursor.execute("""
    CREATE VIEW IF NOT EXISTS plc_keys AS
        SELECT
	        did,
	        created_at,
	        json_extract(operation, '$.services.atproto_pds.endpoint') AS endpoint,
	        json_extract(operation, '$.verificationMethods.atproto') AS key
        FROM plc_latest
    """)

    conn.commit()
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
            INSERT INTO plc_operations (did, cid, nullified, created_at, operation)
            VALUES (?, ?, ?, ?, ?)
            """,
            (
                op.get("did"),
                op.get("cid"),
                op.get("nullified"),
                op.get("createdAt"),
                json.dumps(op.get("operation")),
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
    request_count = 0

    try:
        print("Starting PLC Directory API crawl...")

        while True:
            operations = fetch_plc_operations(session, after)
            request_count += 1

            if not operations:
                print("No more operations to fetch or API error occurred.")
                break

            insert_operations(conn, operations)
            total_processed += len(operations)

            # Get the last timestamp for the next request
            last_op = operations[-1]
            after = last_op.get("createdAt")

            # Progress reporting
            print(
                f"Request #{request_count}: Fetched {len(operations)}, "
                f"Total {total_processed}, Last timestamp: {after}"
            )

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
