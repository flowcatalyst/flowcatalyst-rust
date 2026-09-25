// Stays hand-rolled: BFF endpoint — deliberately stripped from the OpenAPI
// spec (StripBFFPaths), so no generated types exist for it.
import { bffFetch } from "./client";
import type {
	Permission,
	PermissionListResponse,
	CreatePermissionRequest,
} from "@/types/bff";

// Re-export types for consumers
export type { Permission, PermissionListResponse, CreatePermissionRequest };

export const permissionsApi = {
	// list returns the permission catalogue. Pass an application code to scope
	// it to that application's permissions (e.g. when editing one of its
	// roles); omit it for the full catalogue across every application.
	list(application?: string): Promise<PermissionListResponse> {
		const query = application
			? `?application=${encodeURIComponent(application)}`
			: "";
		return bffFetch(`/roles/permissions${query}`);
	},

	get(permission: string): Promise<Permission> {
		return bffFetch(`/roles/permissions/${encodeURIComponent(permission)}`);
	},

	// create defines a permission in the persistent catalogue (idempotent by
	// code). Anchor-gated server-side — anyone who can manage roles can create
	// permissions. Returns the created permission.
	create(req: CreatePermissionRequest): Promise<Permission> {
		return bffFetch(`/roles/permissions`, {
			method: "POST",
			body: JSON.stringify(req),
		});
	},
};
