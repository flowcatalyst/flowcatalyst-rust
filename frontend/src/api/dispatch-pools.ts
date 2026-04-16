import { apiFetch } from "./client";

export type DispatchPoolStatus = "ACTIVE" | "SUSPENDED" | "ARCHIVED";

export interface DispatchPool {
	id: string;
	code: string;
	name: string;
	description?: string;
	rateLimit: number;
	concurrency: number;
	clientId?: string;
	clientIdentifier?: string;
	status: DispatchPoolStatus;
	createdAt: string;
	updatedAt: string;
}

export interface DispatchPoolListResponse {
	pools: DispatchPool[];
	total: number;
}

export interface CreateDispatchPoolRequest {
	code: string;
	name: string;
	description?: string;
	rateLimit: number;
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

export interface StatusResponse {
	message: string;
	poolId: string;
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

	create(data: CreateDispatchPoolRequest): Promise<DispatchPool> {
		return apiFetch("/dispatch-pools", {
			method: "POST",
			body: JSON.stringify(data),
		});
	},

	update(id: string, data: UpdateDispatchPoolRequest): Promise<DispatchPool> {
		return apiFetch(`/dispatch-pools/${id}`, {
			method: "PUT",
			body: JSON.stringify(data),
		});
	},

	delete(id: string): Promise<StatusResponse> {
		return apiFetch(`/dispatch-pools/${id}`, {
			method: "DELETE",
		});
	},

	suspend(id: string): Promise<StatusResponse> {
		return apiFetch(`/dispatch-pools/${id}/suspend`, {
			method: "POST",
		});
	},

	activate(id: string): Promise<StatusResponse> {
		return apiFetch(`/dispatch-pools/${id}/activate`, {
			method: "POST",
		});
	},
};
