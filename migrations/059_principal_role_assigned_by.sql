-- Who assigned a role: iam_principal_roles.assigned_by, the assigning
-- principal's id.
--
-- Go documents `assignedBy` on a service account's role assignments
-- (`RoleAssignmentDTO`, `GET/PUT /api/service-accounts/{id}/roles`) but the
-- junction has no column for it, so it is never populated. This platform
-- stores the executing principal when an administrator assigns a role
-- (service account or principal) or when provisioning grants one. Roles
-- written by the identity-provider or SDK syncs, the bootstrap, and every
-- row written before this column leave it NULL (absent on the wire).
--
-- ADD COLUMN IF NOT EXISTS: re-running is a no-op.
ALTER TABLE iam_principal_roles
    ADD COLUMN IF NOT EXISTS assigned_by VARCHAR(17);
