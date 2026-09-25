import { apiFetch } from "./client";
import type {
	CreatedResponse,
	DispatchPoolListResponse as GenDispatchPoolListResponse,
	DispatchPoolResponse,
} from "./generated";

// Request-side string union the forms/filters rely on. The generated
// response types deliberately stay `string` (the spec doesn't carry enums —
// see docs/frontend-api-types-adoption.md on SDK coordination).
export type DispatchPoolStatus = "ACTIVE" | "SUSPENDED" | "ARCHIVED";

// Response types alias the generated contract (api/openapi.lock.json) so
// `vue-tsc` fails on backend drift. Aliased under the historical names so
// pages keep their imports. `rateLimit` is absent (not `null`) on the wire
// for concurrency-only pools.
export type DispatchPool = DispatchPoolResponse;
export type DispatchPoolListResponse = GenDispatchPoolListResponse;

export interface CreateDispatchPoolRequest {
	code: string;
	name: string;
	description?: string;
	/** Optional. Omit/undefined for concurrency-only pools. */
	rateLimit?: number;
	concurrency: number;
	clientId?: string;
}

export interface UpdateDispatchPoolRequest {
	name?: string;
	description?: string;
	rateLimit?: number;
	concurrency?: number;
	status?: DispatchPoolStatus;
}

export interface DispatchPoolFilters {
	clientId?: string;
	status?: DispatchPoolStatus;
	anchorLevel?: boolean;
}

export const dispatchPoolsApi = {
	list(filters: DispatchPoolFilters = {}): Promise<DispatchPoolListResponse> {
		const params = new URLSearchParams();
		if (filters.clientId) params.set("clientId", filters.clientId);
		if (filters.status) params.set("status", filters.status);
		if (filters.anchorLevel !== undefined)
			params.set("anchorLevel", String(filters.anchorLevel));

		const query = params.toString();
		return apiFetch(`/dispatch-pools${query ? `?${query}` : ""}`);
	},

	get(id: string): Promise<DispatchPool> {
		return apiFetch(`/dispatch-pools/${id}`);
	},

	/** POST /dispatch-pools returns the standard created envelope `{ id }`, not the full pool. */
	create(data: CreateDispatchPoolRequest): Promise<CreatedResponse> {
		return apiFetch("/dispatch-pools", {
			method: "POST",
			body: JSON.stringify(data),
		});
	},

	update(id: string, data: UpdateDispatchPoolRequest): Promise<void> {
		return apiFetch(`/dispatch-pools/${id}`, {
			method: "PUT",
			body: JSON.stringify(data),
		});
	},

	// delete/suspend/activate return 204 No Content on the wire; the old
	// `{ message, poolId }` envelope never existed.
	delete(id: string): Promise<void> {
		return apiFetch(`/dispatch-pools/${id}`, {
			method: "DELETE",
		});
	},

	suspend(id: string): Promise<void> {
		return apiFetch(`/dispatch-pools/${id}/suspend`, {
			method: "POST",
		});
	},

	activate(id: string): Promise<void> {
		return apiFetch(`/dispatch-pools/${id}/activate`, {
			method: "POST",
		});
	},
};
