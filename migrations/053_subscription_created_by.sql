-- Go's 035 (persist-boundary columns), the subscription half: who created a
-- subscription, returned as `createdBy` by GET /api/subscriptions/{id}.
-- Additive and nullable; a database Go migrated already has it.
ALTER TABLE msg_subscriptions ADD COLUMN IF NOT EXISTS created_by VARCHAR(17);
