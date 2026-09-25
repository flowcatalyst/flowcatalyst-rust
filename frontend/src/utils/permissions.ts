/**
 * Permission matching, as the platform does it
 * (`crates/fc-platform/src/role/entity.rs` `matches_pattern`, used by
 * `AuthContext::has_permission`): a held code grants a required one when the
 * two are equal, or when both have exactly four `:`-separated levels
 * (`subdomain:context:aggregate:action`) and every level of the held code is
 * `*` or equal to the required level. So `platform:*:*:*` grants
 * `platform:iam:user:view`, `platform:messaging:*:view` grants every
 * messaging read, and a bare `*` grants only a permission literally named
 * `*` (the platform has none).
 *
 * The server enforces every permission regardless; the SPA uses these only to
 * decide what it shows.
 */

/** Whether the held `pattern` grants the required `permission`. */
export function matchesPattern(permission: string, pattern: string): boolean {
	const required = permission.split(":");
	const held = pattern.split(":");
	if (required.length !== 4 || held.length !== 4) return false;
	return held.every((level, i) => level === "*" || level === required[i]);
}

/** Whether `held` (a user's effective permissions) grants `permission`. */
export function hasPermission(held: readonly string[], permission: string): boolean {
	return held.some((code) => code === permission || matchesPattern(permission, code));
}

/** Whether `held` grants at least one of `permissions` (none: `false`). */
export function hasAnyPermission(held: readonly string[], permissions: readonly string[]): boolean {
	return permissions.some((permission) => hasPermission(held, permission));
}
