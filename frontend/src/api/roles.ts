// Stays hand-rolled: BFF endpoint — deliberately stripped from the OpenAPI
// spec (StripBFFPaths), so no generated types exist for it.
import { bffFetch } from "./client";
import type {
	Role,
	RoleSource,
	RoleListResponse,
	ApplicationOption,
	ApplicationOptionsResponse,
	CreateRoleRequest,
	UpdateRoleRequest,
} from "@/types/bff";

// Re-export types for consumers
export type {
	Role,
	RoleSource,
	RoleListResponse,
	ApplicationOption,
	ApplicationOptionsResponse,
	CreateRoleRequest,
	UpdateRoleRequest,
};

// Frontend-only filter type for API function params
export interface RoleFilters {
	application?: string;
	source?: RoleSource;
}

export const rolesApi = {
	list(filters: RoleFilters = {}): Promise<RoleListResponse> {
		const params = new URLSearchParams();
		if (filters.application) params.set("application", filters.application);
		if (filters.source) params.set("source", filters.source);

		const query = params.toString();
		return bffFetch(`/roles${query ? `?${query}` : ""}`);
	},

	get(roleName: string): Promise<Role> {
		return bffFetch(`/roles/${encodeURIComponent(roleName)}`);
	},

	create(data: CreateRoleRequest): Promise<Role> {
		return bffFetch("/roles", {
			method: "POST",
			body: JSON.stringify(data),
		});
	},

	update(roleName: string, data: UpdateRoleRequest): Promise<Role> {
		return bffFetch(`/roles/${encodeURIComponent(roleName)}`, {
			method: "PUT",
			body: JSON.stringify(data),
		});
	},

	delete(roleName: string): Promise<void> {
		return bffFetch(`/roles/${encodeURIComponent(roleName)}`, {
			method: "DELETE",
		});
	},

	// Filter options
	getApplications(): Promise<ApplicationOptionsResponse> {
		return bffFetch("/roles/filters/applications");
	},

	syncPlatform(): Promise<SyncPlatformRolesResponse> {
		return bffFetch("/roles/sync-platform", { method: "POST" });
	},
};

export interface SyncPlatformRolesResponse {
	created: number;
	updated: number;
	removed: number;
	total: number;
}
