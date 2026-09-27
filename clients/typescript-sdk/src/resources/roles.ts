/**
 * Roles Resource
 *
 * Manage roles and permissions.
 */

import type { ResultAsync } from "neverthrow";
import type { SdkError } from "../errors.js";
import type { FlowCatalystClient } from "../client.js";
import * as sdk from "../generated/sdk.gen.js";
import type {
	ListRolesResponse,
	GetRoleResponse,
	GetRoleByCodeResponse,
	CreateRoleData,
	CreatedResponse,
	UpdateRoleData,
	GetRolesByApplicationResponse,
	SyncRolesData,
	SyncRolesResponse as SyncRolesResponseType,
} from "../generated/types.gen.js";

/**
 * Pagination params (page/size).
 *
 * @deprecated The platform's `GET /api/roles` does not paginate; `list`
 * ignores this argument.
 */
export type PaginationParams = {
	page?: number;
	size?: number;
};

export type RoleListResponse = ListRolesResponse;
export type RoleDto = GetRoleResponse;
export type RoleByCodeResponse = GetRoleByCodeResponse;
export type CreateRoleRequest = CreateRoleData["body"];
export type UpdateRoleRequest = UpdateRoleData["body"];
export type RoleListByApplicationResponse = GetRolesByApplicationResponse;
export type SyncRolesResponse = SyncRolesResponseType;
export type CreateRoleResponse = CreatedResponse;

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
	list(_pagination?: PaginationParams): ResultAsync<RoleListResponse, SdkError> {
		return this.client.request<RoleListResponse>((httpClient, headers) =>
			sdk.listRoles({
				client: httpClient,
				headers,
			}),
		);
	}

	/**
	 * Get a role by name (code) or ID.
	 */
	get(roleName: string): ResultAsync<RoleDto, SdkError> {
		return this.client.request<RoleDto>((httpClient, headers) =>
			sdk.getRole({
				client: httpClient,
				headers,
				path: { id: roleName },
			}),
		);
	}

	/**
	 * Get a role by code (`application:role-name`).
	 */
	getByCode(code: string): ResultAsync<RoleByCodeResponse, SdkError> {
		return this.client.request<RoleByCodeResponse>((httpClient, headers) =>
			sdk.getRoleByCode({
				client: httpClient,
				headers,
				path: { code },
			}),
		);
	}

	/**
	 * Create a new role. The platform answers `201 { id }`; call `get(id)`
	 * for the full entity.
	 */
	create(data: CreateRoleRequest): ResultAsync<CreateRoleResponse, SdkError> {
		return this.client.request<CreateRoleResponse>((httpClient, headers) =>
			sdk.createRole({
				client: httpClient,
				headers,
				body: data,
			}),
		);
	}

	/**
	 * Update a role. The platform answers `204 No Content`; call
	 * `get(roleName)` to read the result.
	 */
	update(
		roleName: string,
		data: UpdateRoleRequest,
	): ResultAsync<void, SdkError> {
		return this.client
			.request<unknown>((httpClient, headers) =>
				sdk.updateRole({
					client: httpClient,
					headers,
					path: { id: roleName },
					body: data,
				}),
			)
			.map((): void => undefined);
	}

	/**
	 * Delete a role.
	 */
	delete(roleName: string): ResultAsync<unknown, SdkError> {
		return this.client.request<unknown>((httpClient, headers) =>
			sdk.deleteRole({
				client: httpClient,
				headers,
				path: { id: roleName },
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
				sdk.getRolesByApplication({
					client: httpClient,
					headers,
					path: { applicationId },
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
			sdk.grantRolePermissionByBody({
				client: httpClient,
				headers,
				path: { roleName },
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
			sdk.revokeRolePermission({
				client: httpClient,
				headers,
				path: { roleName, permission },
			}),
		);
	}

	/**
	 * Sync roles for an application — declarative reconciliation against
	 * `POST /api/applications/{applicationCode}/roles/sync`.
	 */
	sync(
		applicationCode: string,
		roles: SyncRolesData["body"]["roles"],
		removeUnlisted = false,
	): ResultAsync<SyncRolesResponse, SdkError> {
		return this.client.request<SyncRolesResponse>((httpClient, headers) =>
			sdk.syncRoles({
				client: httpClient,
				headers,
				path: { appCode: applicationCode },
				body: { roles },
				query: { removeUnlisted },
			}),
		);
	}
}
