#!/usr/bin/env bash
# Relay-safe, resumable backfill of plc_keys from an existing plc_operations table.
#
# Derives current PDS routing (latest non-nullified op per DID) and writes it into
# plc_keys, matching resolver::derive_pds_key. Splits the work into per-prefix buckets
# over the did:plc: base32 alphabet so each transaction is small: SQLite has a single
# writer, and processing the whole table in one statement would block the live relay's
# export writes for the duration. Idempotent (INSERT OR REPLACE keyed on did) and safe
# to re-run or resume after interruption.
#
# Usage: backfill_plc_keys.sh /data/relay/plc_directory.db
set -euo pipefail

DB="${1:?usage: backfill_plc_keys.sh <plc_directory.db>}"
ALPHABET="234567abcdefghijklmnopqrstuvwxyz"

run_bucket() {
    local ch="$1"
    sqlite3 "$DB" <<SQL
.timeout 600000
INSERT OR REPLACE INTO plc_keys (did, pds_endpoint, pds_key)
SELECT did,
       json_extract(operation, '\$.services.atproto_pds.endpoint'),
       json_extract(operation, '\$.verificationMethods.atproto')
FROM (
    SELECT did, operation,
           ROW_NUMBER() OVER (PARTITION BY did ORDER BY created_at DESC) AS rn
    FROM plc_operations
    WHERE nullified = 0 AND substr(did, 9, 1) = '$ch'
)
WHERE rn = 1
  AND json_extract(operation, '\$.type') = 'plc_operation'
  AND json_extract(operation, '\$.verificationMethods.atproto') IS NOT NULL;

INSERT OR REPLACE INTO plc_keys (did, pds_endpoint, pds_key)
SELECT did,
       json_extract(operation, '\$.service'),
       json_extract(operation, '\$.signingKey')
FROM (
    SELECT did, operation,
           ROW_NUMBER() OVER (PARTITION BY did ORDER BY created_at DESC) AS rn
    FROM plc_operations
    WHERE nullified = 0 AND substr(did, 9, 1) = '$ch'
)
WHERE rn = 1
  AND json_extract(operation, '\$.type') = 'create'
  AND json_extract(operation, '\$.signingKey') IS NOT NULL;

DELETE FROM plc_keys
WHERE did IN (
    SELECT did FROM (
        SELECT did, operation,
               ROW_NUMBER() OVER (PARTITION BY did ORDER BY created_at DESC) AS rn
        FROM plc_operations
        WHERE nullified = 0 AND substr(did, 9, 1) = '$ch'
    )
    WHERE rn = 1 AND json_extract(operation, '\$.type') = 'plc_tombstone'
);
SQL
}

for ((i = 0; i < ${#ALPHABET}; i++)); do
    ch="${ALPHABET:$i:1}"
    printf '[%s] bucket %s ... ' "$(date -u +%H:%M:%S)" "$ch"
    run_bucket "$ch"
    printf 'plc_keys=%s\n' "$(sqlite3 "$DB" 'SELECT count(*) FROM plc_keys;')"
done
echo "backfill complete"
