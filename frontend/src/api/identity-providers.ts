import { apiFetch } from "./client";
import type {
	IdentityProviderListResponse as GenIdentityProviderListResponse,
	IdentityProviderResponse,
} from "./generated";

// Request-side string union the forms rely on. The generated response type
// deliberately stays `string` (the spec doesn't carry enums — see
// docs/frontend-api-types-adoption.md on SDK coordination).
export type IdentityProviderType = "INTERNAL" | "OIDC";

// Scope of the domain mappings created/linked by this request. Required
// whenever the request would create a NEW domain mapping (server: 400
// MAPPING_SCOPE_REQUIRED). See docs product ruling 2026-09-15: the scope
// must never silently fall through to ANCHOR.
export type MappingScope = "ANCHOR" | "CLIENT";

// Response types alias the generated contract (api/openapi.lock.json) so
// `vue-tsc` fails on backend drift. Aliased under the historical names so
// pages keep their imports.
export type IdentityProvider = IdentityProviderResponse;
export type IdentityProviderListResponse = GenIdentityProviderListResponse;

export interface CreateIdentityProviderRequest {
	code: string;
	name: string;
	type: IdentityProviderType;
	oidcIssuerUrl?: string;
	oidcClientId?: string;
	oidcClientSecretRef?: string;
	oidcMultiTenant?: boolean;
	oidcIssuerPattern?: string;
	// Domains listed here are materialized as email-domain mappings: created
	// when unknown, re-pointed (claimed) when already mapped elsewhere.
	allowedEmailDomains?: string[];
	// Linked on mappings that are new or have no primary client yet; an
	// existing client link is never overwritten.
	primaryClientId?: string;
	// Required whenever allowedEmailDomains would create a new domain
	// mapping. CLIENT requires primaryClientId; ANCHOR forbids it.
	mappingScope?: MappingScope;
	syncRolesFromIdp?: boolean;
	allowedRoleIds?: string[];
}

export interface UpdateIdentityProviderRequest {
	name?: string;
	oidcIssuerUrl?: string;
	oidcClientId?: string;
	oidcClientSecretRef?: string;
	oidcMultiTenant?: boolean;
	oidcIssuerPattern?: string;
	// Desired set of domains routed to this provider. Additions are
	// mapped/claimed; removals fall back to internal auth (password).
	allowedEmailDomains?: string[];
	primaryClientId?: string;
	// Required whenever allowedEmailDomains would create a new domain
	// mapping. CLIENT requires primaryClientId; ANCHOR forbids it.
	mappingScope?: MappingScope;
	syncRolesFromIdp?: boolean;
	allowedRoleIds?: string[];
}

export const identityProvidersApi = {
	list(): Promise<IdentityProviderListResponse> {
		return apiFetch("/identity-providers");
	},

	get(id: string): Promise<IdentityProvider> {
		return apiFetch(`/identity-providers/${id}`);
	},

	// NOTE: there is no GET /identity-providers/by-code/{code} on the wire —
	// the previous getByCode() here called a route the backend never exposed
	// (404). Removed when adopting the generated types; use list() + filter
	// or get(id) instead.

	// Unlike most create endpoints (which return `{ id }`), the backend
	// deliberately returns the full provider on 201 so the SPA can render it
	// without a re-fetch (see CreateIdentityProviderResponses in the spec).
	create(
		data: CreateIdentityProviderRequest,
		opts?: { suppressGlobalErrorToast?: boolean },
	): Promise<IdentityProvider> {
		return apiFetch("/identity-providers", {
			method: "POST",
			body: JSON.stringify(data),
			...opts,
		});
	},

	// PUT returns the full updated provider (200), same SPA-friendly choice
	// as create.
	update(
		id: string,
		data: UpdateIdentityProviderRequest,
		opts?: { suppressGlobalErrorToast?: boolean },
	): Promise<IdentityProvider> {
		return apiFetch(`/identity-providers/${id}`, {
			method: "PUT",
			body: JSON.stringify(data),
			...opts,
		});
	},

	delete(id: string): Promise<void> {
		return apiFetch(`/identity-providers/${id}`, {
			method: "DELETE",
		});
	},
};
