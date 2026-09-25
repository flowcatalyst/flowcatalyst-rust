import { apiFetch } from "./client";
import type {
	CreatedResponse,
	MappingListResponse,
	MappingResponse,
} from "./generated";

// Request-side string unions the forms rely on. The generated response type
// deliberately stays `string` (the spec doesn't carry enums — see
// docs/frontend-api-types-adoption.md on SDK coordination).
export type ScopeType = "ANCHOR" | "PARTNER" | "CLIENT";

export type TwoFactorMethod = "TOTP" | "EMAIL_PIN";

// Response types alias the generated contract (api/openapi.lock.json) so
// `vue-tsc` fails on backend drift. Aliased under the historical names so
// pages keep their imports.
//
// Drift fixed when adopting the generated types: the old hand-rolled
// interface claimed `identityProviderType` and `primaryClientName` fields —
// the wire never sends either (the backend DTO only enriches
// `identityProviderName`). Pages must resolve client names from the clients
// list (the detail page already does).
export type EmailDomainMapping = MappingResponse;
export type EmailDomainMappingListResponse = MappingListResponse;

export interface CreateEmailDomainMappingRequest {
	emailDomain: string;
	identityProviderId: string;
	scopeType: ScopeType;
	primaryClientId?: string;
	additionalClientIds?: string[];
	grantedClientIds?: string[];
	requiredOidcTenantId?: string;
	require2fa?: boolean;
	allowed2faMethods?: TwoFactorMethod[];
	rememberDeviceEnabled?: boolean;
	rememberDeviceDays?: number;
}

// Role-sync config (allowedRoleIds / syncRolesFromIdp) lives on the identity
// provider, not the mapping. Re-pointing a mapping to another provider goes
// through moveProvider(), not update().
export interface UpdateEmailDomainMappingRequest {
	primaryClientId?: string;
	additionalClientIds?: string[];
	grantedClientIds?: string[];
	requiredOidcTenantId?: string;
	require2fa?: boolean;
	allowed2faMethods?: TwoFactorMethod[];
	rememberDeviceEnabled?: boolean;
	rememberDeviceDays?: number;
}

export interface EmailDomainMappingSearchParams {
	identityProviderId?: string;
	scopeType?: ScopeType;
}

// Note: the backend also exposes GET /email-domain-mappings/lookup?domain=…
// (used by the login flow); its response is untyped on the wire (`unknown`
// in the generated types), so it is intentionally not surfaced here.
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

	// POST returns the standard created envelope `{ id }` (201), not the full
	// mapping. To display the full mapping, re-fetch via `get(id)`.
	create(data: CreateEmailDomainMappingRequest): Promise<CreatedResponse> {
		return apiFetch("/email-domain-mappings", {
			method: "POST",
			body: JSON.stringify(data),
		});
	},

	// PUT returns 204 No Content — no body. Callers should re-fetch via get(id)
	// if they need the updated record.
	update(id: string, data: UpdateEmailDomainMappingRequest): Promise<void> {
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

	// Re-points the domain to a different identity provider via the move use
	// case. Moving to an internal provider converts the domain's
	// OIDC-provisioned users back to internal auth (usersReset reports how
	// many); moving to an OIDC provider flips routing at the next login.
	moveProvider(
		id: string,
		identityProviderId: string,
	): Promise<MoveProviderResponse> {
		return apiFetch(`/email-domain-mappings/${id}/move-provider`, {
			method: "POST",
			body: JSON.stringify({ identityProviderId }),
		});
	},
};

export interface MoveProviderResponse {
	mappingId: string;
	emailDomain: string;
	fromIdentityProviderId: string;
	toIdentityProviderId: string;
	usersReset: number;
}
