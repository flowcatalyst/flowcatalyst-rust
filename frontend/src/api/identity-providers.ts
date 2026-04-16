import { apiFetch } from "./client";

export type IdentityProviderType = "INTERNAL" | "OIDC";

export interface IdentityProvider {
	id: string;
	code: string;
	name: string;
	type: IdentityProviderType;
	oidcIssuerUrl?: string;
	oidcClientId?: string;
	oidcMultiTenant: boolean;
	oidcIssuerPattern?: string;
	allowedEmailDomains: string[];
	hasClientSecret: boolean;
	createdAt: string;
	updatedAt: string;
}

export interface IdentityProviderListResponse {
	identityProviders: IdentityProvider[];
	total: number;
}

export interface CreateIdentityProviderRequest {
	code: string;
	name: string;
	type: IdentityProviderType;
	oidcIssuerUrl?: string;
	oidcClientId?: string;
	oidcClientSecretRef?: string;
	oidcMultiTenant?: boolean;
	oidcIssuerPattern?: string;
	allowedEmailDomains?: string[];
}

export interface UpdateIdentityProviderRequest {
	name?: string;
	oidcIssuerUrl?: string;
	oidcClientId?: string;
	oidcClientSecretRef?: string;
	oidcMultiTenant?: boolean;
	oidcIssuerPattern?: string;
	allowedEmailDomains?: string[];
}

export const identityProvidersApi = {
	list(): Promise<IdentityProviderListResponse> {
		return apiFetch("/identity-providers");
	},

	get(id: string): Promise<IdentityProvider> {
		return apiFetch(`/identity-providers/${id}`);
	},

	getByCode(code: string): Promise<IdentityProvider> {
		return apiFetch(
			`/identity-providers/by-code/${encodeURIComponent(code)}`,
		);
	},

	create(data: CreateIdentityProviderRequest): Promise<IdentityProvider> {
		return apiFetch("/identity-providers", {
			method: "POST",
			body: JSON.stringify(data),
		});
	},

	update(
		id: string,
		data: UpdateIdentityProviderRequest,
	): Promise<IdentityProvider> {
		return apiFetch(`/identity-providers/${id}`, {
			method: "PUT",
			body: JSON.stringify(data),
		});
	},

	delete(id: string): Promise<void> {
		return apiFetch(`/identity-providers/${id}`, {
			method: "DELETE",
		});
	},
};
