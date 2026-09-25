import { apiFetch } from "./client";
import type {
	ApplicationListResponse as GenApplicationListResponse,
	ApplicationLoginClientCredentials,
	ApplicationProvisionLoginClientResponse,
	ApplicationProvisionServiceAccountResponse,
	ApplicationResponse,
	ApplicationServiceAccountCredentials,
	CreatedResponse,
} from "./generated";

// Request-side string union the forms rely on. The generated response
// types deliberately stay `string` (the spec doesn't carry enums — see
// docs/frontend-api-types-adoption.md on SDK coordination).
export type ApplicationType = "APPLICATION" | "INTEGRATION";

// Response types alias the generated contract (api/openapi.lock.json) so
// `vue-tsc` fails on backend drift. Aliased under the historical names so
// pages keep their imports. Note `hasLoginClient` is always on the wire
// (required boolean); list responses leave it false — only the detail
// endpoint computes it.
export type Application = ApplicationResponse;
export type ApplicationListResponse = GenApplicationListResponse;
export type ServiceAccountCredentials = ApplicationServiceAccountCredentials;
export type LoginClientCredentials = ApplicationLoginClientCredentials;

export interface CreateApplicationRequest {
	code: string;
	name: string;
	description?: string;
	defaultBaseUrl?: string;
	iconUrl?: string;
	website?: string;
	logo?: string;
	logoMimeType?: string;
	type?: ApplicationType; // Defaults to APPLICATION
}

export interface UpdateApplicationRequest {
	name?: string;
	description?: string;
	defaultBaseUrl?: string;
	iconUrl?: string;
	website?: string;
	logo?: string;
	logoMimeType?: string;
}

export interface ListApplicationsOptions {
	activeOnly?: boolean;
	type?: ApplicationType;
}

export const applicationsApi = {
	list(
		options: ListApplicationsOptions = {},
	): Promise<ApplicationListResponse> {
		const params = new URLSearchParams();
		if (options.activeOnly) params.append("activeOnly", "true");
		if (options.type) params.append("type", options.type);
		const queryString = params.toString();
		return apiFetch(`/applications${queryString ? `?${queryString}` : ""}`);
	},

	/**
	 * List only user-facing applications (type = APPLICATION).
	 * Use this when populating selectors for assigning apps to clients/users.
	 */
	listApplicationsOnly(activeOnly = true): Promise<ApplicationListResponse> {
		return this.list({ activeOnly, type: "APPLICATION" });
	},

	/**
	 * List only integrations (type = INTEGRATION).
	 */
	listIntegrationsOnly(activeOnly = true): Promise<ApplicationListResponse> {
		return this.list({ activeOnly, type: "INTEGRATION" });
	},

	get(id: string): Promise<Application> {
		return apiFetch(`/applications/${id}`);
	},

	getByCode(code: string): Promise<Application> {
		return apiFetch(`/applications/by-code/${code}`);
	},

	/**
	 * Create a new application or integration.
	 * Returns the standard created envelope `{ id }` — service-account
	 * credentials are NOT issued on create; use `provisionServiceAccount`.
	 */
	create(data: CreateApplicationRequest): Promise<CreatedResponse> {
		return apiFetch("/applications", {
			method: "POST",
			body: JSON.stringify(data),
		});
	},

	update(id: string, data: UpdateApplicationRequest): Promise<void> {
		return apiFetch(`/applications/${id}`, {
			method: "PUT",
			body: JSON.stringify(data),
		});
	},

	activate(id: string): Promise<Application> {
		return apiFetch(`/applications/${id}/activate`, { method: "POST" });
	},

	deactivate(id: string): Promise<Application> {
		return apiFetch(`/applications/${id}/deactivate`, { method: "POST" });
	},

	delete(id: string): Promise<void> {
		return apiFetch(`/applications/${id}`, { method: "DELETE" });
	},

	/**
	 * Provision a service account for an existing application.
	 * Returns the credentials (only available at provisioning time).
	 */
	provisionServiceAccount(
		id: string,
	): Promise<ApplicationProvisionServiceAccountResponse> {
		return apiFetch(`/applications/${id}/provision-service-account`, {
			method: "POST",
		});
	},

	/**
	 * Provision a user-login OAuth client for an existing application.
	 *
	 * - `PUBLIC` clients (default — SPAs, native apps) enforce PKCE and return
	 *   no `clientSecret`; protect the `clientId` and rely on the PKCE flow.
	 * - `CONFIDENTIAL` clients (server-rendered apps) get a `clientSecret`
	 *   returned exactly once.
	 *
	 * 409 if a login client already exists for the application — rotate the
	 * existing one via the OAuth Clients page, or delete it before
	 * re-provisioning.
	 */
	provisionLoginClient(
		id: string,
		body: ProvisionLoginClientRequest,
	): Promise<ApplicationProvisionLoginClientResponse> {
		return apiFetch(`/applications/${id}/provision-login-client`, {
			method: "POST",
			body: JSON.stringify(body),
		});
	},
};

/** Request body for `provisionLoginClient` (hand-rolled: keeps the
 * clientType union the form binds to; the wire accepts plain string). */
export interface ProvisionLoginClientRequest {
	clientType?: "PUBLIC" | "CONFIDENTIAL";
	redirectUris: string[];
	allowedOrigins?: string[];
}
