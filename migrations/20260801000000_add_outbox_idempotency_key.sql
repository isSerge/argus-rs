-- Add an idempotency key to the outbox so duplicate enqueues (e.g. the ingestor
-- re-fetching overlapping block ranges on fast chains) are dropped before
-- delivery instead of producing duplicate alerts.

ALTER TABLE outbox ADD COLUMN idempotency_key TEXT NOT NULL DEFAULT '';

-- Backfill any pre-existing in-flight rows with unique values so the unique
-- index below can be created without conflict.
UPDATE outbox SET idempotency_key = 'legacy-' || CAST(id AS TEXT) WHERE idempotency_key = '';

CREATE UNIQUE INDEX idx_outbox_idempotency_key ON outbox(idempotency_key);
