/**
 * Connections Resource
 *
 * Manage connections between service accounts and subscription targets.
 *
 * Uses direct HTTP calls against the platform's `/api/connections` routes.
 */

import { okAsync, type ResultAsync } from "neverthrow";
import type { SdkError } from "../errors.js";
import type { FlowCatalystClient } from "../client.js";

export interface ConnectionDto {
	id: string;
	code: string;
	name: string;
	/** Omitted by the platform when unset. */
	description?: string | null;
	/** @deprecated The platform has no connection endpoint; never set. */
	endpoint?: string;
	externalId?: string | null;
	status: string;
	serviceAccountId: string;
	clientId?: string | null;
	clientIdentifier?: string | null;
	/** The application the connection belongs to, when linked. */
	applicationCode?: string;
	/** Where the connection was authored: `CODE` (sync) or `API`. */
	source?: string;
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
	/** @deprecated The platform has no connection endpoint and ignores it. */
	endpoint?: string;
	externalId?: string | null;
	serviceAccountId: string;
	clientId?: string | null;
	/** Link the connection to an application. */
	applicationCode?: string;
}

export interface UpdateConnectionRequest {
	/**
	 * Required by the platform. When omitted, `update` reads the current
	 * name first and sends it unchanged.
	 */
	name?: string;
	description?: string | null;
	/** @deprecated The platform has no connection endpoint and ignores it. */
	endpoint?: string;
	externalId?: string | null;
	status?: "ACTIVE" | "PAUSED";
	/** Link the connection to an application; omitted leaves the link alone. */
	applicationCode?: string;
}

export interface ConnectionFilters {
	clientId?: string;
	status?: string;
	/**
	 * @deprecated The platform's `GET /api/connections` has no
	 * `serviceAccountId` filter and ignores it.
	 */
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
	 * Update a connection. The platform answers `204 No Content`; call
	 * `get(id)` to read the result.
	 *
	 * The platform requires `name`. When it is omitted, the current name is
	 * read first (`get(id)`) and sent unchanged. The platform replaces
	 * `description` and `externalId` (omitting one clears it); an omitted
	 * `status` or `applicationCode` is left as it is.
	 */
	update(id: string, data: UpdateConnectionRequest): ResultAsync<void, SdkError> {
		const body: ResultAsync<UpdateConnectionRequest, SdkError> =
			data.name !== undefined && data.name !== null
				? okAsync(data)
				: this.get(id).map((current) => ({ ...data, name: current.name }));
		return body
			.andThen((b) =>
				this.client.request<unknown>((httpClient, headers) =>
					httpClient.put({
						url: "/api/connections/{id}",
						headers: {
							...headers,
							"Content-Type": "application/json",
						},
						path: { id },
						body: b,
					}),
				),
			)
			.map((): void => undefined);
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
