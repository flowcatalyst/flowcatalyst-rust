/**
 * Principals Resource
 *
 * Manage users and service accounts.
 */

import type { ResultAsync } from "neverthrow";
import type { SdkError } from "../errors";
import type { FlowCatalystClient } from "../client";
import * as sdk from "../generated/sdk.gen";
import type {
	GetApiAdminPrincipalsResponse,
	GetApiAdminPrincipalsByIdResponse,
	PostApiAdminPrincipalsUsersData,
	PutApiAdminPrincipalsByIdData,
	PostApiAdminPrincipalsByIdResetPasswordData,
	GetApiAdminPrincipalsByIdRolesResponse,
	GetApiAdminPrincipalsByIdClientAccessResponse,
} from "../generated/types.gen";

export type PrincipalListResponse = GetApiAdminPrincipalsResponse;
export type PrincipalDto = GetApiAdminPrincipalsByIdResponse;
export type CreateUserRequest = PostApiAdminPrincipalsUsersData["body"];
export type UpdatePrincipalRequest = PutApiAdminPrincipalsByIdData["body"];
export type ResetPasswordRequest =
	PostApiAdminPrincipalsByIdResetPasswordData["body"];
export type RoleListResponse = GetApiAdminPrincipalsByIdRolesResponse;
export type ClientAccessListResponse =
	GetApiAdminPrincipalsByIdClientAccessResponse;

export interface PrincipalFilters {
	clientId?: string;
	type?: string;
	active?: string;
	email?: string;
}

/**
 * Principals resource for managing users and service accounts.
 */
export class PrincipalsResource {
	private readonly client: FlowCatalystClient;

	constructor(client: FlowCatalystClient) {
		this.client = client;
	}

	/**
	 * List all principals with optional filters.
	 */
	list(
		filters?: PrincipalFilters,
	): ResultAsync<PrincipalListResponse, SdkError> {
		return this.client.request<PrincipalListResponse>((httpClient, headers) =>
			sdk.getApiAdminPrincipals({
				client: httpClient,
				headers,
				query: filters,
			}),
		);
	}

	/**
	 * Get a principal by ID.
	 */
	get(id: string): ResultAsync<PrincipalDto, SdkError> {
		return this.client.request<PrincipalDto>((httpClient, headers) =>
			sdk.getApiAdminPrincipalsById({
				client: httpClient,
				headers,
				path: { id },
			}),
		);
	}

	/**
	 * Find a user by email.
	 */
	findByEmail(email: string): ResultAsync<PrincipalListResponse, SdkError> {
		return this.list({ email });
	}

	/**
	 * Create a new user principal.
	 */
	createUser(data: CreateUserRequest): ResultAsync<PrincipalDto, SdkError> {
		return this.client.request<PrincipalDto>((httpClient, headers) =>
			sdk.postApiAdminPrincipalsUsers({
				client: httpClient,
				headers,
				body: data,
			}),
		);
	}

	/**
	 * Update a principal.
	 */
	update(
		id: string,
		data: UpdatePrincipalRequest,
	): ResultAsync<PrincipalDto, SdkError> {
		return this.client.request<PrincipalDto>((httpClient, headers) =>
			sdk.putApiAdminPrincipalsById({
				client: httpClient,
				headers,
				path: { id },
				body: data,
			}),
		);
	}

	/**
	 * Activate a principal.
	 */
	activate(id: string): ResultAsync<PrincipalDto, SdkError> {
		return this.client.request<PrincipalDto>((httpClient, headers) =>
			sdk.postApiAdminPrincipalsByIdActivate({
				client: httpClient,
				headers,
				path: { id },
			}),
		);
	}

	/**
	 * Deactivate a principal.
	 */
	deactivate(id: string): ResultAsync<PrincipalDto, SdkError> {
		return this.client.request<PrincipalDto>((httpClient, headers) =>
			sdk.postApiAdminPrincipalsByIdDeactivate({
				client: httpClient,
				headers,
				path: { id },
			}),
		);
	}

	/**
	 * Reset a user's password.
	 *
	 * Set `enforcePasswordComplexity` on `data` to `false` when the caller
	 * enforces its own password policy; only the platform's 2-character
	 * minimum will apply. Defaults to `true`.
	 */
	resetPassword(
		id: string,
		data: ResetPasswordRequest,
	): ResultAsync<unknown, SdkError> {
		return this.client.request<unknown>((httpClient, headers) =>
			sdk.postApiAdminPrincipalsByIdResetPassword({
				client: httpClient,
				headers,
				path: { id },
				body: data,
			}),
		);
	}

	/**
	 * Get roles assigned to a principal.
	 */
	getRoles(id: string): ResultAsync<RoleListResponse, SdkError> {
		return this.client.request<RoleListResponse>((httpClient, headers) =>
			sdk.getApiAdminPrincipalsByIdRoles({
				client: httpClient,
				headers,
				path: { id },
			}),
		);
	}

	/**
	 * Assign a single role to a principal.
	 */
	assignRole(id: string, roleName: string): ResultAsync<unknown, SdkError> {
		return this.client.request<unknown>((httpClient, headers) =>
			sdk.postApiAdminPrincipalsByIdRoles({
				client: httpClient,
				headers,
				path: { id },
				body: { role: roleName },
			}),
		);
	}

	/**
	 * Remove a role from a principal.
	 */
	removeRole(id: string, roleName: string): ResultAsync<unknown, SdkError> {
		return this.client.request<unknown>((httpClient, headers) =>
			sdk.deleteApiAdminPrincipalsByIdRolesByRoleName({
				client: httpClient,
				headers,
				path: { id, role: roleName },
			}),
		);
	}

	/**
	 * Assign roles to a principal (declarative - replaces all roles).
	 */
	assignRoles(id: string, roles: string[]): ResultAsync<unknown, SdkError> {
		return this.client.request<unknown>((httpClient, headers) =>
			sdk.putApiAdminPrincipalsByIdRoles({
				client: httpClient,
				headers,
				path: { id },
				body: { roles },
			}),
		);
	}

	/**
	 * Get client access grants for a principal.
	 */
	getClientAccessGrants(
		id: string,
	): ResultAsync<ClientAccessListResponse, SdkError> {
		return this.client.request<ClientAccessListResponse>(
			(httpClient, headers) =>
				sdk.getApiAdminPrincipalsByIdClientAccess({
					client: httpClient,
					headers,
					path: { id },
				}),
		);
	}

	/**
	 * Grant client access to a principal.
	 */
	grantClientAccess(
		id: string,
		clientId: string,
	): ResultAsync<unknown, SdkError> {
		return this.client.request<unknown>((httpClient, headers) =>
			sdk.postApiAdminPrincipalsByIdClientAccess({
				client: httpClient,
				headers,
				path: { id },
				body: { clientId },
			}),
		);
	}

	/**
	 * Revoke client access from a principal.
	 */
	revokeClientAccess(
		id: string,
		clientId: string,
	): ResultAsync<unknown, SdkError> {
		return this.client.request<unknown>((httpClient, headers) =>
			sdk.deleteApiAdminPrincipalsByIdClientAccessByClientId({
				client: httpClient,
				headers,
				path: { id, client_id: clientId },
			}),
		);
	}
}
