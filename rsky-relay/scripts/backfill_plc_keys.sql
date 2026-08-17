-- One-time backfill of plc_keys from an existing plc_operations table.
--
-- The relay derives PDS routing from plc_keys (SELECT ... WHERE did = ?). Before the
-- key-derivation fix, plc_operations was populated but plc_keys was never written, so
-- every lookup missed and no identity validation happened. This reconstructs plc_keys
-- from the operation history already on disk: the latest non-nullified operation per DID
-- wins, matching the going-forward logic in resolver::derive_pds_key.
--
-- Safe to re-run (idempotent: INSERT OR REPLACE keyed on did). Run with the relay's PLC
-- export paused or in a maintenance window: this holds SQLite's single writer lock for
-- the duration of the INSERT, and the export path also writes.

.timeout 600000

-- Modern `plc_operation` ops: routing lives in nested services/verificationMethods.
INSERT OR REPLACE INTO plc_keys (did, pds_endpoint, pds_key)
SELECT did,
       json_extract(operation, '$.services.atproto_pds.endpoint'),
       json_extract(operation, '$.verificationMethods.atproto')
FROM (
    SELECT did, operation,
           ROW_NUMBER() OVER (PARTITION BY did ORDER BY created_at DESC) AS rn
    FROM plc_operations
    WHERE nullified = 0
)
WHERE rn = 1
  AND json_extract(operation, '$.type') = 'plc_operation'
  AND json_extract(operation, '$.verificationMethods.atproto') IS NOT NULL;

-- Legacy genesis `create` ops (flat shape) for accounts that never issued a later op.
INSERT OR REPLACE INTO plc_keys (did, pds_endpoint, pds_key)
SELECT did,
       json_extract(operation, '$.service'),
       json_extract(operation, '$.signingKey')
FROM (
    SELECT did, operation,
           ROW_NUMBER() OVER (PARTITION BY did ORDER BY created_at DESC) AS rn
    FROM plc_operations
    WHERE nullified = 0
)
WHERE rn = 1
  AND json_extract(operation, '$.type') = 'create'
  AND json_extract(operation, '$.signingKey') IS NOT NULL;

-- Drop routing for accounts whose latest op is a tombstone.
DELETE FROM plc_keys
WHERE did IN (
    SELECT did FROM (
        SELECT did, operation,
               ROW_NUMBER() OVER (PARTITION BY did ORDER BY created_at DESC) AS rn
        FROM plc_operations
        WHERE nullified = 0
    )
    WHERE rn = 1
      AND json_extract(operation, '$.type') = 'plc_tombstone'
);
