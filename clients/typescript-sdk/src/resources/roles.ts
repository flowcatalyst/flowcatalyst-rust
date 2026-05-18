/**
 * Roles Resource
 *
 * Manage roles and permissions.
 */

import type { ResultAsync } from "neverthrow";
import type { SdkError } from "../errors";
import type { FlowCatalystClient } from "../client";
import * as sdk from "../generated/sdk.gen";
import type {
	GetApiRolesResponse,
	GetApiRolesByNameResponse,
	GetApiRolesByCodeByCodeResponse,
	PostApiRolesData,
	PutApiRolesByNameData,
	GetApiRolesByApplicationByApplicationIdResponse,
	PostApiApplicationsByAppCodeRolesSyncData,
	PostApiApplicationsByAppCodeRolesSyncResponse,
	PaginationParams,
} from "../generated/types.gen";

export type RoleListResponse = GetApiRolesResponse;
export type RoleDto = GetApiRolesByNameResponse;
export type RoleByCodeResponse = GetApiRolesByCodeByCodeResponse;
export type CreateRoleRequest = PostApiRolesData["body"];
export type UpdateRoleRequest = PutApiRolesByNameData["body"];
export type RoleListByApplicationResponse =
	GetApiRolesByApplicationByApplicationIdResponse;
export type SyncRolesResponse = PostApiApplicationsByAppCodeRolesSyncResponse;

/**
 * Roles resource for managing role-based access control.
 */
export class RolesResource {
	private readonly client: FlowCatalystClient;

	constructor(client: FlowCatalystClient) {
		this.client = client;
	}

	/**
	 * List all roles.
	 */
	list(pagination?: PaginationParams): ResultAsync<RoleListResponse, SdkError> {
		return this.client.request<RoleListResponse>((httpClient, headers) =>
			sdk.getApiRoles({
				client: httpClient,
				headers,
				query: { pagination: pagination ?? {} },
			}),
		);
	}

	/**
	 * Get a role by name.
	 */
	get(roleName: string): ResultAsync<RoleDto, SdkError> {
		return this.client.request<RoleDto>((httpClient, headers) =>
			sdk.getApiRolesByName({
				client: httpClient,
				headers,
				path: { role_name: roleName },
			}),
		);
	}

	/**
	 * Get a role by code (`application:role-name`).
	 */
	getByCode(code: string): ResultAsync<RoleByCodeResponse, SdkError> {
		return this.client.request<RoleByCodeResponse>((httpClient, headers) =>
			sdk.getApiRolesByCodeByCode({
				client: httpClient,
				headers,
				path: { code },
			}),
		);
	}

	/**
	 * Create a new role.
	 */
	create(data: CreateRoleRequest): ResultAsync<RoleDto, SdkError> {
		return this.client.request<RoleDto>((httpClient, headers) =>
			sdk.postApiRoles({
				client: httpClient,
				headers,
				body: data,
			}),
		);
	}

	/**
	 * Update a role.
	 */
	update(
		roleName: string,
		data: UpdateRoleRequest,
	): ResultAsync<RoleDto, SdkError> {
		return this.client.request<RoleDto>((httpClient, headers) =>
			sdk.putApiRolesByName({
				client: httpClient,
				headers,
				path: { role_name: roleName },
				body: data,
			}),
		);
	}

	/**
	 * Delete a role.
	 */
	delete(roleName: string): ResultAsync<unknown, SdkError> {
		return this.client.request<unknown>((httpClient, headers) =>
			sdk.deleteApiRolesByName({
				client: httpClient,
				headers,
				path: { role_name: roleName },
			}),
		);
	}

	/**
	 * List roles for an application.
	 */
	listForApplication(
		applicationId: string,
	): ResultAsync<RoleListByApplicationResponse, SdkError> {
		return this.client.request<RoleListByApplicationResponse>(
			(httpClient, headers) =>
				sdk.getApiRolesByApplicationByApplicationId({
					client: httpClient,
					headers,
					path: { application_id: applicationId },
				}),
		);
	}

	/**
	 * Grant a permission to a role. Returns the updated role.
	 */
	grantPermission(
		roleName: string,
		permission: string,
	): ResultAsync<RoleDto, SdkError> {
		return this.client.request<RoleDto>((httpClient, headers) =>
			sdk.postApiRolesByNamePermissions({
				client: httpClient,
				headers,
				path: { role_name: roleName },
				body: { permission },
			}),
		);
	}

	/**
	 * Revoke a permission from a role. Returns the updated role.
	 */
	revokePermission(
		roleName: string,
		permission: string,
	): ResultAsync<RoleDto, SdkError> {
		return this.client.request<RoleDto>((httpClient, headers) =>
			sdk.deleteApiRolesByNamePermissionsByPermission({
				client: httpClient,
				headers,
				path: { role_name: roleName, permission },
			}),
		);
	}

	/**
	 * Sync roles for an application — declarative reconciliation against
	 * `POST /api/applications/{applicationCode}/roles/sync`.
	 */
	sync(
		applicationCode: string,
		roles: PostApiApplicationsByAppCodeRolesSyncData["body"]["roles"],
		removeUnlisted = false,
	): ResultAsync<SyncRolesResponse, SdkError> {
		return this.client.request<SyncRolesResponse>((httpClient, headers) =>
			sdk.postApiApplicationsByAppCodeRolesSync({
				client: httpClient,
				headers,
				path: { app_code: applicationCode },
				body: { roles },
				query: { removeUnlisted },
			}),
		);
	}
}
