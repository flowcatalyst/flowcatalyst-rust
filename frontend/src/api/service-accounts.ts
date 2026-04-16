import { apiFetch } from "./client";
import type { PrincipalScope } from "./users";

export type WebhookAuthType = "BEARER" | "BASIC";

export interface ServiceAccount {
	id: string;
	code: string;
	name: string;
	description: string | null;
	scope: PrincipalScope | null;
	clientIds: string[];
	applicationId: string | null;
	active: boolean;
	authType: WebhookAuthType | null;
	roles: string[];
	lastUsedAt: string | null;
	createdAt: string;
	updatedAt: string;
}

export interface ServiceAccountListResponse {
	serviceAccounts: ServiceAccount[];
	total: number;
}

export interface CreateServiceAccountRequest {
	code: string;
	name: string;
	description?: string;
	clientIds?: string[];
	applicationId?: string;
	scope?: PrincipalScope;
}

export interface OAuthCredentials {
	clientId: string;
	clientSecret: string;
}

export interface WebhookCredentials {
	authToken: string;
	signingSecret: string;
}

export interface CreateServiceAccountResponse {
	serviceAccount: ServiceAccount;
	principalId: string;
	oauth: OAuthCredentials;
	webhook: WebhookCredentials;
}

export interface UpdateServiceAccountRequest {
	name?: string;
	description?: string;
	clientIds?: string[];
	scope?: PrincipalScope;
}

export interface RegenerateTokenResponse {
	authToken: string;
}

export interface RegenerateSecretResponse {
	signingSecret: string;
}

export interface RoleAssignment {
	roleName: string;
	assignmentSource: string;
	assignedAt: string;
}

export interface RolesResponse {
	roles: RoleAssignment[];
}

export interface RolesAssignedResponse {
	roles: RoleAssignment[];
	addedRoles: string[];
	removedRoles: string[];
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
	 * Update a service account's metadata.
	 */
	update(
		id: string,
		data: UpdateServiceAccountRequest,
	): Promise<ServiceAccount> {
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
	 * Update the auth token with a custom value.
	 */
	updateAuthToken(id: string, authToken: string): Promise<ServiceAccount> {
		return apiFetch(`/service-accounts/${id}/auth-token`, {
			method: "PUT",
			body: JSON.stringify({ authToken }),
		});
	},

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
};
