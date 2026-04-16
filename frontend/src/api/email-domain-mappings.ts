import { apiFetch } from "./client";

export type ScopeType = "ANCHOR" | "PARTNER" | "CLIENT";

export interface EmailDomainMapping {
	id: string;
	emailDomain: string;
	identityProviderId: string;
	identityProviderName?: string;
	identityProviderType?: string;
	scopeType: ScopeType;
	primaryClientId?: string;
	primaryClientName?: string;
	additionalClientIds: string[];
	grantedClientIds: string[];
	requiredOidcTenantId?: string;
	allowedRoleIds: string[];
	syncRolesFromIdp: boolean;
	createdAt: string;
	updatedAt: string;
}

export interface EmailDomainMappingListResponse {
	mappings: EmailDomainMapping[];
	total: number;
}

export interface CreateEmailDomainMappingRequest {
	emailDomain: string;
	identityProviderId: string;
	scopeType: ScopeType;
	primaryClientId?: string;
	additionalClientIds?: string[];
	grantedClientIds?: string[];
	requiredOidcTenantId?: string;
	allowedRoleIds?: string[];
	syncRolesFromIdp?: boolean;
}

export interface UpdateEmailDomainMappingRequest {
	scopeType?: ScopeType;
	primaryClientId?: string;
	additionalClientIds?: string[];
	grantedClientIds?: string[];
	requiredOidcTenantId?: string;
	allowedRoleIds?: string[];
	syncRolesFromIdp?: boolean;
}

export interface EmailDomainMappingSearchParams {
	identityProviderId?: string;
	scopeType?: ScopeType;
}

export const emailDomainMappingsApi = {
	list(
		params?: EmailDomainMappingSearchParams,
	): Promise<EmailDomainMappingListResponse> {
		const searchParams = new URLSearchParams();
		if (params?.identityProviderId)
			searchParams.set("identityProviderId", params.identityProviderId);
		if (params?.scopeType) searchParams.set("scopeType", params.scopeType);
		const queryString = searchParams.toString();
		return apiFetch(
			`/email-domain-mappings${queryString ? `?${queryString}` : ""}`,
		);
	},

	get(id: string): Promise<EmailDomainMapping> {
		return apiFetch(`/email-domain-mappings/${id}`);
	},

	getByDomain(domain: string): Promise<EmailDomainMapping> {
		return apiFetch(
			`/email-domain-mappings/by-domain/${encodeURIComponent(domain)}`,
		);
	},

	create(data: CreateEmailDomainMappingRequest): Promise<EmailDomainMapping> {
		return apiFetch("/email-domain-mappings", {
			method: "POST",
			body: JSON.stringify(data),
		});
	},

	update(
		id: string,
		data: UpdateEmailDomainMappingRequest,
	): Promise<EmailDomainMapping> {
		return apiFetch(`/email-domain-mappings/${id}`, {
			method: "PUT",
			body: JSON.stringify(data),
		});
	},

	delete(id: string): Promise<void> {
		return apiFetch(`/email-domain-mappings/${id}`, {
			method: "DELETE",
		});
	},
};
