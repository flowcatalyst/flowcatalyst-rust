/**
 * Permissions Resource
 *
 * Query available permissions.
 */

import type { ResultAsync } from "neverthrow";
import type { SdkError } from "../errors.js";
import type { FlowCatalystClient } from "../client.js";
import * as sdk from "../generated/sdk.gen.js";
import type {
	ListPermissionsResponse,
	GetPermissionResponse,
} from "../generated/types.gen.js";

export type PermissionListResponse = ListPermissionsResponse;
export type PermissionDto = GetPermissionResponse;

/**
 * Permissions resource for querying available permissions.
 */
export class PermissionsResource {
	private readonly client: FlowCatalystClient;

	constructor(client: FlowCatalystClient) {
		this.client = client;
	}

	/**
	 * List all permissions.
	 */
	list(): ResultAsync<PermissionListResponse, SdkError> {
		return this.client.request<PermissionListResponse>((httpClient, headers) =>
			sdk.listPermissions({
				client: httpClient,
				headers,
			}),
		);
	}

	/**
	 * Get a permission by name.
	 */
	get(permission: string): ResultAsync<PermissionDto, SdkError> {
		return this.client.request<PermissionDto>((httpClient, headers) =>
			sdk.getPermission({
				client: httpClient,
				headers,
				path: { permission },
			}),
		);
	}
}
