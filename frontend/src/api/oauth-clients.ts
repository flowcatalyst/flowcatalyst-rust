import { apiFetch } from "./client";

export type ClientType = "PUBLIC" | "CONFIDENTIAL";

export interface ApplicationRef {
	id: string;
	name: string;
}

export interface OAuthClient {
	id: string;
	clientId: string;
	clientName: string;
	clientType: ClientType;
	redirectUris: string[];
	allowedOrigins: string[];
	grantTypes: string[];
	defaultScopes: string[];
	pkceRequired: boolean;
	applicationIds: string[];
	applications?: ApplicationRef[];
	active: boolean;
	createdAt: string;
	updatedAt: string;
}

export interface OAuthClientListResponse {
	clients: OAuthClient[];
	total: number;
}

export interface CreateOAuthClientRequest {
	clientName: string;
	clientType: ClientType;
	redirectUris: string[];
	allowedOrigins?: string[];
	grantTypes: string[];
	defaultScopes?: string;
	pkceRequired?: boolean;
	applicationIds?: string[];
}

export interface UpdateOAuthClientRequest {
	clientName?: string;
	redirectUris?: string[];
	allowedOrigins?: string[];
	grantTypes?: string[];
	defaultScopes?: string[];
	pkceRequired?: boolean;
	applicationIds?: string[];
}

export interface CreateOAuthClientResponse {
	client: OAuthClient;
	/** Plaintext secret for CONFIDENTIAL clients — shown only once at creation time */
	clientSecret?: string;
}

export interface RotateSecretResponse {
	clientId: string;
	clientSecret: string;
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

	getByClientId(clientId: string): Promise<OAuthClient> {
		return apiFetch(`/oauth-clients/by-client-id/${clientId}`);
	},

	create(data: CreateOAuthClientRequest): Promise<CreateOAuthClientResponse> {
		return apiFetch("/oauth-clients", {
			method: "POST",
			body: JSON.stringify(data),
		});
	},

	update(id: string, data: UpdateOAuthClientRequest): Promise<OAuthClient> {
		return apiFetch(`/oauth-clients/${id}`, {
			method: "PUT",
			body: JSON.stringify(data),
		});
	},

	rotateSecret(id: string): Promise<RotateSecretResponse> {
		return apiFetch(`/oauth-clients/${id}/rotate-secret`, {
			method: "POST",
		});
	},

	activate(id: string): Promise<{ message: string }> {
		return apiFetch(`/oauth-clients/${id}/activate`, { method: "POST" });
	},

	deactivate(id: string): Promise<{ message: string }> {
		return apiFetch(`/oauth-clients/${id}/deactivate`, {
			method: "POST",
		});
	},

	delete(id: string): Promise<void> {
		return apiFetch(`/oauth-clients/${id}`, { method: "DELETE" });
	},
};
