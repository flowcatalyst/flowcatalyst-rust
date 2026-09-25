/**
 * Tag severity for a role assignment's `assignmentSource`, using the values
 * the API emits: `ADMIN_ASSIGNED`, `IDP_SYNC`, `SDK_SYNC`, `PROVISIONED`,
 * `BOOTSTRAP`, `SYSTEM`, and `ADMIN` for legacy rows with no recorded source.
 *
 * Roles an admin assigned by hand stand out; roles that sync or the platform
 * granted are muted.
 */
export function assignmentSourceSeverity(source: string): "info" | "secondary" {
	return source === "ADMIN_ASSIGNED" || source === "ADMIN" ? "info" : "secondary";
}
