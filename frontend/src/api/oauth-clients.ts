import { apiFetch } from "./client";
import type {
	CreateOAuthClientResponse as GenCreateOAuthClientResponse,
	OAuthClientApplicationRef,
	OAuthClientListResponse as GenOAuthClientListResponse,
	OAuthClientResponse,
	RotateOAuthClientSecretResponse,
	SuccessResponse,
} from "./generated";

// Request-side string union the forms rely on. The generated response
// types deliberately stay `string` (the spec doesn't carry enums — see
// docs/frontend-api-types-adoption.md on SDK coordination).
export type ClientType = "PUBLIC" | "CONFIDENTIAL";

// Response types alias the generated contract (api/openapi.lock.json) so
// `vue-tsc` fails on backend drift. Aliased under the historical names so
// pages keep their imports.
export type ApplicationRef = OAuthClientApplicationRef;
export type OAuthClient = OAuthClientResponse;
export type OAuthClientListResponse = GenOAuthClientListResponse;
export type CreateOAuthClientResponse = GenCreateOAuthClientResponse;
export type RotateSecretResponse = RotateOAuthClientSecretResponse;

export interface CreateOAuthClientRequest {
	clientName: string;
	clientType: ClientType;
	redirectUris: string[];
	postLogoutRedirectUris?: string[];
	allowedOrigins?: string[];
	grantTypes: string[];
	defaultScopes?: string[];
	pkceRequired?: boolean;
	applicationIds?: string[];
	// Marks the client as a portal entry point owned by this tenant client.
	portalClientId?: string;
	// Links the portal client to one of that client's portal apps.
	portalAppId?: string;
	// Trusted first-party opt-in: interactive logins mint authority-bearing
	// access tokens narrowed to the client's applications.
	apiAccess?: boolean;
}

export interface UpdateOAuthClientRequest {
	clientName?: string;
	redirectUris?: string[];
	postLogoutRedirectUris?: string[];
	allowedOrigins?: string[];
	grantTypes?: string[];
	defaultScopes?: string[];
	pkceRequired?: boolean;
	applicationIds?: string[];
	// Empty string clears the portal flag; omitted keeps it.
	portalClientId?: string;
	// Empty string unlinks the portal app; omitted keeps it.
	portalAppId?: string;
	apiAccess?: boolean;
}

export const oauthClientsApi = {
	list(params?: {
		applicationId?: string;
		active?: boolean;
	}): Promise<OAuthClientListResponse> {
		const searchParams = new URLSearchParams();
		if (params?.applicationId)
			searchParams.set("applicationId", params.applicationId);
		if (params?.active !== undefined)
			searchParams.set("active", String(params.active));
		const query = searchParams.toString();
		return apiFetch(`/oauth-clients${query ? "?" + query : ""}`);
	},

	get(id: string): Promise<OAuthClient> {
		return apiFetch(`/oauth-clients/${id}`);
	},

	getByClientId(
		clientId: string,
		opts?: { suppressGlobalErrorToast?: boolean; suppressAuthErrorEvent?: boolean },
	): Promise<OAuthClient> {
		return apiFetch(`/oauth-clients/by-client-id/${clientId}`, opts);
	},

	create(data: CreateOAuthClientRequest): Promise<CreateOAuthClientResponse> {
		return apiFetch("/oauth-clients", {
			method: "POST",
			body: JSON.stringify(data),
		});
	},

	update(
		id: string,
		data: UpdateOAuthClientRequest,
		opts?: { suppressGlobalErrorToast?: boolean },
	): Promise<void> {
		return apiFetch(`/oauth-clients/${id}`, {
			method: "PUT",
			body: JSON.stringify(data),
			...opts,
		});
	},

	rotateSecret(id: string): Promise<RotateSecretResponse> {
		return apiFetch(`/oauth-clients/${id}/rotate-secret`, {
			method: "POST",
		});
	},

	activate(id: string): Promise<SuccessResponse> {
		return apiFetch(`/oauth-clients/${id}/activate`, { method: "POST" });
	},

	deactivate(id: string): Promise<SuccessResponse> {
		return apiFetch(`/oauth-clients/${id}/deactivate`, {
			method: "POST",
		});
	},

	delete(id: string): Promise<void> {
		return apiFetch(`/oauth-clients/${id}`, { method: "DELETE" });
	},
};
