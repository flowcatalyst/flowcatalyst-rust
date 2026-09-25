import { apiFetch } from "./client";
import type {
	ConnectionListResponse as GenConnectionListResponse,
	ConnectionResponse,
} from "./generated";

// Request-side string union the filters rely on. The generated response
// types deliberately stay `string` (the spec doesn't carry enums — see
// docs/frontend-api-types-adoption.md on SDK coordination).
export type ConnectionStatus = "ACTIVE" | "PAUSED";

// Response types alias the generated contract (api/openapi.lock.json) so
// `vue-tsc` fails on backend drift. Aliased under the historical names so
// pages keep their imports.
export type Connection = ConnectionResponse;
export type ConnectionListResponse = GenConnectionListResponse;

export interface CreateConnectionRequest {
	code: string;
	name: string;
	description?: string;
	externalId?: string;
	serviceAccountId: string;
	clientId?: string;
}

export interface UpdateConnectionRequest {
	name?: string;
	description?: string;
	externalId?: string;
}

export interface ConnectionFilters {
	clientId?: string;
	status?: ConnectionStatus;
}

export const connectionsApi = {
	list(filters: ConnectionFilters = {}): Promise<ConnectionListResponse> {
		const params = new URLSearchParams();
		if (filters.clientId) params.set("clientId", filters.clientId);
		if (filters.status) params.set("status", filters.status);

		const query = params.toString();
		return apiFetch(`/connections${query ? `?${query}` : ""}`);
	},

	get(id: string): Promise<Connection> {
		return apiFetch(`/connections/${id}`);
	},

	create(data: CreateConnectionRequest): Promise<Connection> {
		return apiFetch("/connections", {
			method: "POST",
			body: JSON.stringify(data),
		});
	},

	update(id: string, data: UpdateConnectionRequest): Promise<void> {
		return apiFetch(`/connections/${id}`, {
			method: "PUT",
			body: JSON.stringify(data),
		});
	},

	delete(id: string): Promise<void> {
		return apiFetch(`/connections/${id}`, {
			method: "DELETE",
		});
	},

	/** Returns the updated connection (the wire sends the full entity, not a status envelope). */
	pause(id: string): Promise<Connection> {
		return apiFetch(`/connections/${id}/pause`, {
			method: "POST",
		});
	},

	/** Returns the updated connection (the wire sends the full entity, not a status envelope). */
	activate(id: string): Promise<Connection> {
		return apiFetch(`/connections/${id}/activate`, {
			method: "POST",
		});
	},
};
