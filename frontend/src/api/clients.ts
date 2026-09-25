import { apiFetch } from "./client";
import type {
	ClientApplicationResponse,
	ClientApplicationsResponse as GenClientApplicationsResponse,
	ClientListResponse as GenClientListResponse,
	ClientResponse,
	CreatedResponse,
	StatusChangeResponse,
} from "./generated";

// Response types alias the generated contract (api/openapi.lock.json) so
// `vue-tsc` fails on backend drift. Aliased under the historical names so
// pages keep their imports. The generated `status` stays plain `string`
// (the spec doesn't carry enums — see docs/frontend-api-types-adoption.md).
export type Client = ClientResponse;
export type ClientListResponse = GenClientListResponse;
export type ClientApplication = ClientApplicationResponse;
export type ClientApplicationsResponse = GenClientApplicationsResponse;

export interface CreateClientRequest {
	name: string;
	identifier: string;
}

export interface UpdateClientRequest {
	name: string;
}

export interface ClientSearchParams {
	q?: string;
	status?: string;
	limit?: number;
}

export const clientsApi = {
	list(
		params?: { page?: number; pageSize?: number; status?: string } | string,
	): Promise<ClientListResponse> {
		const searchParams = new URLSearchParams();
		if (typeof params === "string") {
			if (params) searchParams.set("status", params);
		} else if (params) {
			if (params.page !== undefined)
				searchParams.set("page", String(params.page));
			if (params.pageSize !== undefined)
				searchParams.set("pageSize", String(params.pageSize));
			if (params.status) searchParams.set("status", params.status);
		}
		const query = searchParams.toString();
		return apiFetch(`/clients${query ? `?${query}` : ""}`);
	},

	search(params: ClientSearchParams = {}): Promise<ClientListResponse> {
		const searchParams = new URLSearchParams();
		if (params.q) searchParams.set("q", params.q);
		if (params.status) searchParams.set("status", params.status);
		if (params.limit) searchParams.set("limit", String(params.limit));
		const queryString = searchParams.toString();
		return apiFetch(`/clients/search${queryString ? `?${queryString}` : ""}`);
	},

	get(id: string): Promise<Client> {
		return apiFetch(`/clients/${id}`);
	},

	getByIdentifier(identifier: string): Promise<Client> {
		return apiFetch(`/clients/by-identifier/${identifier}`);
	},

	/** POST /clients returns the standard created envelope `{ id }`, not the full client. */
	create(data: CreateClientRequest): Promise<CreatedResponse> {
		return apiFetch("/clients", {
			method: "POST",
			body: JSON.stringify(data),
		});
	},

	update(id: string, data: UpdateClientRequest): Promise<void> {
		return apiFetch(`/clients/${id}`, {
			method: "PUT",
			body: JSON.stringify(data),
		});
	},

	activate(id: string): Promise<StatusChangeResponse> {
		return apiFetch(`/clients/${id}/activate`, {
			method: "POST",
		});
	},

	suspend(id: string, reason: string): Promise<StatusChangeResponse> {
		return apiFetch(`/clients/${id}/suspend`, {
			method: "POST",
			body: JSON.stringify({ reason }),
		});
	},

	deactivate(id: string, reason: string): Promise<StatusChangeResponse> {
		return apiFetch(`/clients/${id}/deactivate`, {
			method: "POST",
			body: JSON.stringify({ reason }),
		});
	},

	addNote(
		id: string,
		category: string,
		text: string,
	): Promise<StatusChangeResponse> {
		return apiFetch(`/clients/${id}/notes`, {
			method: "POST",
			body: JSON.stringify({ category, text }),
		});
	},

	// Application management
	getApplications(clientId: string): Promise<ClientApplicationsResponse> {
		return apiFetch(`/clients/${clientId}/applications`);
	},

	enableApplication(clientId: string, applicationId: string): Promise<void> {
		return apiFetch(
			`/clients/${clientId}/applications/${applicationId}/enable`,
			{
				method: "POST",
			},
		);
	},

	disableApplication(clientId: string, applicationId: string): Promise<void> {
		return apiFetch(
			`/clients/${clientId}/applications/${applicationId}/disable`,
			{
				method: "POST",
			},
		);
	},

	updateApplications(
		clientId: string,
		enabledApplicationIds: string[],
	): Promise<void> {
		return apiFetch(`/clients/${clientId}/applications`, {
			method: "PUT",
			body: JSON.stringify({ enabledApplicationIds }),
		});
	},
};
