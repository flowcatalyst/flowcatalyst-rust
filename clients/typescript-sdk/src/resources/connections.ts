/**
 * Connections Resource
 *
 * Manage connections between service accounts and subscription targets.
 *
 * Uses direct HTTP calls since generated SDK functions are not yet available
 * (OpenAPI spec not regenerated). Will be migrated to generated functions
 * once the spec is updated.
 */

import type { ResultAsync } from "neverthrow";
import type { SdkError } from "../errors";
import type { FlowCatalystClient } from "../client";

export interface ConnectionDto {
	id: string;
	code: string;
	name: string;
	description: string | null;
	endpoint: string;
	externalId: string | null;
	status: string;
	serviceAccountId: string;
	clientId: string | null;
	clientIdentifier: string | null;
	createdAt: string;
	updatedAt: string;
}

export interface ConnectionListResponse {
	connections: ConnectionDto[];
	total: number;
}

export interface CreateConnectionRequest {
	code: string;
	name: string;
	description?: string | null;
	endpoint: string;
	externalId?: string | null;
	serviceAccountId: string;
	clientId?: string | null;
}

export interface UpdateConnectionRequest {
	name?: string;
	description?: string | null;
	endpoint?: string;
	externalId?: string | null;
	status?: "ACTIVE" | "PAUSED";
}

export interface ConnectionFilters {
	clientId?: string;
	status?: string;
	serviceAccountId?: string;
	[key: string]: unknown;
}

/**
 * Connections resource for managing service-account-to-target connections.
 */
export class ConnectionsResource {
	private readonly client: FlowCatalystClient;

	constructor(client: FlowCatalystClient) {
		this.client = client;
	}

	/**
	 * List all connections with optional filters.
	 */
	list(
		filters?: ConnectionFilters,
	): ResultAsync<ConnectionListResponse, SdkError> {
		return this.client.request<ConnectionListResponse>(
			(httpClient, headers) =>
				httpClient.get({
					url: "/api/connections",
					headers,
					query: filters,
				}),
		);
	}

	/**
	 * Get a connection by ID.
	 */
	get(id: string): ResultAsync<ConnectionDto, SdkError> {
		return this.client.request<ConnectionDto>((httpClient, headers) =>
			httpClient.get({
				url: "/api/connections/{id}",
				headers,
				path: { id },
			}),
		);
	}

	/**
	 * Create a new connection.
	 */
	create(
		data: CreateConnectionRequest,
	): ResultAsync<ConnectionDto, SdkError> {
		return this.client.request<ConnectionDto>((httpClient, headers) =>
			httpClient.post({
				url: "/api/connections",
				headers: {
					...headers,
					"Content-Type": "application/json",
				},
				body: data,
			}),
		);
	}

	/**
	 * Update a connection.
	 */
	update(
		id: string,
		data: UpdateConnectionRequest,
	): ResultAsync<ConnectionDto, SdkError> {
		return this.client.request<ConnectionDto>((httpClient, headers) =>
			httpClient.put({
				url: "/api/connections/{id}",
				headers: {
					...headers,
					"Content-Type": "application/json",
				},
				path: { id },
				body: data,
			}),
		);
	}

	/**
	 * Delete a connection.
	 */
	delete(id: string): ResultAsync<unknown, SdkError> {
		return this.client.request<unknown>((httpClient, headers) =>
			httpClient.delete({
				url: "/api/connections/{id}",
				headers,
				path: { id },
			}),
		);
	}

	/**
	 * Pause a connection.
	 */
	pause(id: string): ResultAsync<ConnectionDto, SdkError> {
		return this.client.request<ConnectionDto>((httpClient, headers) =>
			httpClient.post({
				url: "/api/connections/{id}/pause",
				headers,
				path: { id },
			}),
		);
	}

	/**
	 * Activate a connection.
	 */
	activate(id: string): ResultAsync<ConnectionDto, SdkError> {
		return this.client.request<ConnectionDto>((httpClient, headers) =>
			httpClient.post({
				url: "/api/connections/{id}/activate",
				headers,
				path: { id },
			}),
		);
	}
}
