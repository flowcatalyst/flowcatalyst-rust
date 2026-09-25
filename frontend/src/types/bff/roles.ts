/**
 * BFF Role & Permission contracts — local type stubs.
 *
 * These mirror the shapes returned by the BFF endpoints.
 */

export interface BffRole {
	id: string;
	name: string;
	shortName: string;
	displayName: string;
	description?: string;
	permissions: string[];
	applicationCode: string;
	source: "CODE" | "DATABASE" | "SDK";
	clientManaged: boolean;
	createdAt: string;
	updatedAt: string;
}

export interface BffPermission {
	permission: string;
	application: string;
	context: string;
	aggregate: string;
	action: string;
	description: string;
}

export interface BffRoleListResponse {
	items: BffRole[];
	total: number;
}

export interface BffApplicationOption {
	id: string;
	code: string;
	name: string;
}

export interface BffApplicationOptionsResponse {
	options: BffApplicationOption[];
}

export interface BffCreateRoleRequest {
	applicationCode: string;
	roleName: string;
	displayName: string;
	description?: string;
	permissions?: string[];
	clientManaged?: boolean;
}

export interface BffUpdateRoleRequest {
	displayName?: string;
	description?: string;
	permissions?: string[];
	clientManaged?: boolean;
}

export interface BffPermissionListResponse {
	items: BffPermission[];
	total: number;
}

// Body for POST /bff/roles/permissions — the four segments form the canonical
// "application:context:aggregate:action" code; description is optional.
export interface BffCreatePermissionRequest {
	application: string;
	context: string;
	aggregate: string;
	action: string;
	description?: string;
}

// Re-export with aliases
export type {
	BffRole as Role,
	BffPermission as Permission,
	BffRoleListResponse as RoleListResponse,
	BffApplicationOption as ApplicationOption,
	BffApplicationOptionsResponse as ApplicationOptionsResponse,
	BffCreateRoleRequest as CreateRoleRequest,
	BffUpdateRoleRequest as UpdateRoleRequest,
	BffPermissionListResponse as PermissionListResponse,
	BffCreatePermissionRequest as CreatePermissionRequest,
};

// Derived enum type
export type RoleSource = BffRole["source"];
