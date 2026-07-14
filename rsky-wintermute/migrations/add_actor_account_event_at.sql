-- Track the PDS-origin timestamp of the last #account event applied to each actor.
-- Lets process_account_event reject stale events delivered out of order across relays
-- (e.g. old-PDS deactivate arriving after new-PDS activate during a migration).
-- Nullable, no default; old binaries that don't write this column remain compatible.

ALTER TABLE bsky.actor ADD COLUMN IF NOT EXISTS "accountEventAt" timestamptz;
