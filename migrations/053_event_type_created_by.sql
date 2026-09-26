-- msg_event_types.created_by, as Go has it (Go migration 035
-- persist_boundary_columns): the principal that created the event type,
-- answered as `createdBy`. Nullable; a Go-migrated database already has it.
ALTER TABLE msg_event_types ADD COLUMN IF NOT EXISTS created_by VARCHAR(17);
