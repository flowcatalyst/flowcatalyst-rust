/**
 * Who the SPA offers an action the BFF would refuse. Each check is the one the
 * endpoint makes (docs/platform-bug-fixes.md): anchor scope and the permission
 * for the operation. The pages showed Sync Platform Events and the role
 * editors to everyone, and most got a 403.
 */
import { userHasPermission, userScope } from "@/stores/permissions";

type SessionUser =
	| { permissions?: string[]; clientId?: string | null }
	| null
	| undefined;

export const EVENT_TYPE_SYNC = "platform:messaging:event-type:sync";
export const EVENT_TYPE_MANAGE = "platform:messaging:event-type:manage";
export const ROLE_CREATE = "platform:iam:role:create";
export const ROLE_UPDATE = "platform:iam:role:update";
export const ROLE_DELETE = "platform:iam:role:delete";
export const USER_ASSIGN_ROLES = "platform:iam:user:assign-roles";

function isAnchor(user: SessionUser): boolean {
	return !!user && userScope(user) === "anchor";
}

/** POST /bff/event-types/sync-platform: anchor and event-type:sync or :manage. */
export function canSyncPlatformEventTypes(user: SessionUser): boolean {
	return (
		isAnchor(user) &&
		(userHasPermission(user, EVENT_TYPE_SYNC) ||
			userHasPermission(user, EVENT_TYPE_MANAGE))
	);
}

/** POST /bff/roles and POST /bff/roles/permissions: anchor and role:create. */
export function canCreateRoles(user: SessionUser): boolean {
	return isAnchor(user) && userHasPermission(user, ROLE_CREATE);
}

/** PUT /bff/roles/{name}: anchor and role:update. */
export function canUpdateRoles(user: SessionUser): boolean {
	return isAnchor(user) && userHasPermission(user, ROLE_UPDATE);
}

/** DELETE /bff/roles/{name}: anchor and role:delete. */
export function canDeleteRoles(user: SessionUser): boolean {
	return isAnchor(user) && userHasPermission(user, ROLE_DELETE);
}

/**
 * POST /bff/roles/sync-platform creates, updates and removes platform roles:
 * anchor and all three role permissions.
 */
export function canSyncPlatformRoles(user: SessionUser): boolean {
	return canCreateRoles(user) && canUpdateRoles(user) && canDeleteRoles(user);
}

/**
 * PUT and POST /api/principals/{id}/roles, DELETE .../roles/{role}, and the
 * roles of a CSV import row: user:assign-roles, not user:update (owner ruling
 * 2026-10-11). No anchor check: a client administrator holding it assigns its
 * client's application roles, and the API bounds which.
 */
export function canAssignUserRoles(user: SessionUser): boolean {
	return userHasPermission(user, USER_ASSIGN_ROLES);
}
