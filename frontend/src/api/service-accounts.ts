import { apiFetch } from "./client";
import type { PrincipalScope } from "./users";
import type {
	ApplicationAccessListResponse,
	ApplicationAccessResponse,
	CreateServiceAccountResponse as GenCreateServiceAccountResponse,
	PrincipalAvailableApplication,
	PrincipalAvailableApplicationsResponse,
	RegenerateAuthTokenResponse,
	RegenerateSigningSecretResponse,
	RoleAssignmentDto,
	ServiceAccountTokenResponse as GenServiceAccountTokenResponse,
	ServiceAccountListResponse as GenServiceAccountListResponse,
	ServiceAccountOAuthSecrets,
	ServiceAccountResponse,
	ServiceAccountRoleListResponse,
	ServiceAccountRolesAssignedResponse,
	ServiceAccountWebhookSecrets,
	SetApplicationAccessResponse,
} from "./generated";

// Request-side string union the forms rely on. The generated response
// types deliberately stay `string` (the spec doesn't carry enums — see
// docs/frontend-api-types-adoption.md on SDK coordination).
export type WebhookAuthType = "BEARER" | "BASIC";

// Response types alias the generated contract (api/openapi.lock.json) so
// `vue-tsc` fails on backend drift. Aliased under the historical names so
// pages keep their imports. Optional fields come back `undefined` (not
// `null`); normalise at use sites.
export type ServiceAccount = ServiceAccountResponse;
export type ServiceAccountListResponse = GenServiceAccountListResponse;
export type OAuthCredentials = ServiceAccountOAuthSecrets;
export type WebhookCredentials = ServiceAccountWebhookSecrets;
export type CreateServiceAccountResponse = GenCreateServiceAccountResponse;
export type RegenerateTokenResponse = RegenerateAuthTokenResponse;
export type RegenerateSecretResponse = RegenerateSigningSecretResponse;
export type ServiceAccountTokenResponse = GenServiceAccountTokenResponse;
export type RoleAssignment = RoleAssignmentDto;
export type RolesResponse = ServiceAccountRoleListResponse;
export type RolesAssignedResponse = ServiceAccountRolesAssignedResponse;
export type ApplicationAccessGrant = ApplicationAccessResponse;
export type ApplicationAccessAssignedResponse = SetApplicationAccessResponse;
export type AvailableApplication = PrincipalAvailableApplication;
export type AvailableApplicationsResponse =
	PrincipalAvailableApplicationsResponse;

export interface CreateServiceAccountRequest {
	code: string;
	name: string;
	description?: string;
	clientIds?: string[];
	applicationId?: string;
	/** Grant every application. Omitted/false → no application access. */
	allApplications?: boolean;
	scope?: PrincipalScope;
}

export interface UpdateServiceAccountRequest {
	name?: string;
	description?: string;
	clientIds?: string[];
	scope?: PrincipalScope;
}

export interface ServiceAccountFilters {
	clientId?: string;
	applicationId?: string;
	active?: boolean;
}

export const serviceAccountsApi = {
	/**
	 * List all service accounts with optional filters.
	 */
	list(filters?: ServiceAccountFilters): Promise<ServiceAccountListResponse> {
		const params = new URLSearchParams();
		if (filters?.clientId) params.append("clientId", filters.clientId);
		if (filters?.applicationId)
			params.append("applicationId", filters.applicationId);
		if (filters?.active !== undefined)
			params.append("active", String(filters.active));

		const query = params.toString();
		return apiFetch(`/service-accounts${query ? `?${query}` : ""}`);
	},

	/**
	 * Get a service account by ID.
	 */
	get(id: string): Promise<ServiceAccount> {
		return apiFetch(`/service-accounts/${id}`);
	},

	/**
	 * Get a service account by code.
	 */
	getByCode(code: string): Promise<ServiceAccount> {
		return apiFetch(`/service-accounts/code/${code}`);
	},

	/**
	 * Create a new service account.
	 * Returns the created service account along with credentials (shown only once).
	 */
	create(
		data: CreateServiceAccountRequest,
	): Promise<CreateServiceAccountResponse> {
		return apiFetch("/service-accounts", {
			method: "POST",
			body: JSON.stringify(data),
		});
	},

	/**
	 * Update a service account's metadata. Returns 204 (no body) — reload or
	 * patch local state from the request after a successful save.
	 */
	update(id: string, data: UpdateServiceAccountRequest): Promise<void> {
		return apiFetch(`/service-accounts/${id}`, {
			method: "PUT",
			body: JSON.stringify(data),
		});
	},

	/**
	 * Delete a service account.
	 */
	delete(id: string): Promise<void> {
		return apiFetch(`/service-accounts/${id}`, {
			method: "DELETE",
		});
	},

	// ==================== Credential Management ====================

	/**
	 * Regenerate the auth token (returns new token, shown only once).
	 */
	regenerateToken(id: string): Promise<RegenerateTokenResponse> {
		return apiFetch(`/service-accounts/${id}/regenerate-token`, {
			method: "POST",
		});
	},

	/**
	 * Regenerate the signing secret (returns new secret, shown only once).
	 */
	regenerateSecret(id: string): Promise<RegenerateSecretResponse> {
		return apiFetch(`/service-accounts/${id}/regenerate-secret`, {
			method: "POST",
		});
	},

	/**
	 * Mint a short-lived bearer token for the service account (anchor-only,
	 * audited). Same claims as a client_credentials exchange; expires in an
	 * hour and is never shown again — treat it as a live credential.
	 */
	mintToken(id: string): Promise<ServiceAccountTokenResponse> {
		return apiFetch(`/service-accounts/${id}/token`, {
			method: "POST",
		});
	},

	// ==================== Role Management ====================

	/**
	 * Get assigned roles for a service account.
	 */
	getRoles(id: string): Promise<RolesResponse> {
		return apiFetch(`/service-accounts/${id}/roles`);
	},

	/**
	 * Assign roles to a service account (declarative - replaces all existing roles).
	 */
	assignRoles(id: string, roles: string[]): Promise<RolesAssignedResponse> {
		return apiFetch(`/service-accounts/${id}/roles`, {
			method: "PUT",
			body: JSON.stringify({ roles }),
		});
	},

	// ==================== Application Access ====================
	//
	// A service account's roles + application access live on its linked SERVICE
	// principal, not the service-account row, so these target the shared
	// /principals/{principalId}/application-access endpoints. The principal id
	// comes from ServiceAccountResponse.principalId (single-account read).

	/**
	 * Get the application access grants for a service account's principal.
	 */
	getApplicationAccess(
		principalId: string,
	): Promise<ApplicationAccessListResponse> {
		return apiFetch(`/principals/${principalId}/application-access`);
	},

	/**
	 * Get applications available to grant to a service account's principal.
	 */
	getAvailableApplications(
		principalId: string,
	): Promise<AvailableApplicationsResponse> {
		return apiFetch(`/principals/${principalId}/available-applications`);
	},

	/**
	 * Declaratively set a service account's application access. allApplications is
	 * omitted (left unchanged) unless explicitly passed; only an all-applications
	 * administrator may set it true (backend-enforced).
	 */
	assignApplicationAccess(
		principalId: string,
		applicationIds: string[],
		allApplications?: boolean,
	): Promise<SetApplicationAccessResponse> {
		const body: { applicationIds: string[]; allApplications?: boolean } = {
			applicationIds,
		};
		if (allApplications !== undefined) {
			body.allApplications = allApplications;
		}
		return apiFetch(`/principals/${principalId}/application-access`, {
			method: "PUT",
			body: JSON.stringify(body),
		});
	},
};
