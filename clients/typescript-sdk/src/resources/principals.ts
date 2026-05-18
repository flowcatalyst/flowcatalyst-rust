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
	GetApiPrincipalsData,
	GetApiPrincipalsResponse,
	GetApiPrincipalsByIdResponse,
	PostApiPrincipalsUsersData,
	PutApiPrincipalsByIdData,
	PostApiPrincipalsByIdResetPasswordData,
	GetApiPrincipalsByIdRolesResponse,
	GetApiPrincipalsByIdClientAccessResponse,
	PostApiApplicationsByAppCodePrincipalsSyncData,
	PostApiApplicationsByAppCodePrincipalsSyncResponse,
} from "../generated/types.gen";

export type PrincipalListResponse = GetApiPrincipalsResponse;
export type PrincipalDto = GetApiPrincipalsByIdResponse;
export type CreateUserRequest = PostApiPrincipalsUsersData["body"];
export type UpdatePrincipalRequest = PutApiPrincipalsByIdData["body"];
export type ResetPasswordRequest =
	PostApiPrincipalsByIdResetPasswordData["body"];
export type RoleListResponse = GetApiPrincipalsByIdRolesResponse;
export type ClientAccessListResponse =
	GetApiPrincipalsByIdClientAccessResponse;
export type SyncPrincipalsResponse =
	PostApiApplicationsByAppCodePrincipalsSyncResponse;

// Derived from the generated query type so it stays in sync with the platform
// spec automatically — adding a query param upstream surfaces here on regen.
export type PrincipalFilters = GetApiPrincipalsData["query"];

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
			sdk.getApiPrincipals({
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
			sdk.getApiPrincipalsById({
				client: httpClient,
				headers,
				path: { id },
			}),
		);
	}

	/**
	 * Find a user by email.
	 *
	 * Client-side filters the response to rows whose email matches exactly
	 * (case-insensitive). Older platform builds silently ignored unknown
	 * query parameters and returned an unfiltered list; we defend against
	 * that here so callers don't act on the wrong principal.
	 */
	findByEmail(email: string): ResultAsync<PrincipalListResponse, SdkError> {
		const needle = email.toLowerCase();
		return this.list({ email }).map((response) => {
			const principals = response.principals.filter(
				(p) => (p.email ?? "").toLowerCase() === needle,
			);
			return { principals, total: principals.length };
		});
	}

	/**
	 * Create a new user principal.
	 */
	createUser(data: CreateUserRequest): ResultAsync<PrincipalDto, SdkError> {
		return this.client.request<PrincipalDto>((httpClient, headers) =>
			sdk.postApiPrincipalsUsers({
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
			sdk.putApiPrincipalsById({
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
			sdk.postApiPrincipalsByIdActivate({
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
			sdk.postApiPrincipalsByIdDeactivate({
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
			sdk.postApiPrincipalsByIdResetPassword({
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
			sdk.getApiPrincipalsByIdRoles({
				client: httpClient,
				headers,
				path: { id },
			}),
		);
	}

	/**
	 * Add a single role to a principal (additive — keeps existing roles).
	 *
	 * Renamed from `assignRole` to make the additive-vs-replace distinction
	 * visible at the call site (paired with `setRoles` for replace-all).
	 */
	addRole(id: string, roleName: string): ResultAsync<unknown, SdkError> {
		return this.client.request<unknown>((httpClient, headers) =>
			sdk.postApiPrincipalsByIdRoles({
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
			sdk.deleteApiPrincipalsByIdRolesByRoleName({
				client: httpClient,
				headers,
				path: { id, role: roleName },
			}),
		);
	}

	/**
	 * Replace all roles on a principal with the given set (declarative).
	 *
	 * Renamed from `assignRoles` so the replace semantics are obvious
	 * (paired with `addRole` for additive).
	 */
	setRoles(id: string, roles: string[]): ResultAsync<unknown, SdkError> {
		return this.client.request<unknown>((httpClient, headers) =>
			sdk.putApiPrincipalsByIdRoles({
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
				sdk.getApiPrincipalsByIdClientAccess({
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
			sdk.postApiPrincipalsByIdClientAccess({
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
			sdk.deleteApiPrincipalsByIdClientAccessByClientId({
				client: httpClient,
				headers,
				path: { id, client_id: clientId },
			}),
		);
	}

	/**
	 * Sync principals for an application — declarative reconciliation
	 * against `POST /api/applications/{applicationCode}/principals/sync`.
	 *
	 * When `removeUnlisted` is true the platform strips SDK-sourced role
	 * assignments from principals not in the list; principals themselves
	 * are never deleted by sync.
	 */
	sync(
		applicationCode: string,
		principals: PostApiApplicationsByAppCodePrincipalsSyncData["body"]["principals"],
		removeUnlisted = false,
	): ResultAsync<SyncPrincipalsResponse, SdkError> {
		return this.client.request<SyncPrincipalsResponse>(
			(httpClient, headers) =>
				sdk.postApiApplicationsByAppCodePrincipalsSync({
					client: httpClient,
					headers,
					path: { app_code: applicationCode },
					body: { principals },
					query: { removeUnlisted },
				}),
		);
	}
}
