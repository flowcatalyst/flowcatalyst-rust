/**
 * Event Types Resource
 *
 * Manage event type definitions and schemas.
 */

import type { ResultAsync } from "neverthrow";
import type { SdkError } from "../errors";
import type { FlowCatalystClient } from "../client";
import * as sdk from "../generated/sdk.gen";
import type {
	GetApiEventTypesResponse,
	GetApiEventTypesByIdResponse,
	PostApiEventTypesData,
	PutApiEventTypesByIdData,
	PostApiEventTypesByIdSchemasData,
	PostApiApplicationsByAppCodeEventTypesSyncData,
	PostApiApplicationsByAppCodeEventTypesSyncResponse,
	PaginationParams,
} from "../generated/types.gen";

export type EventTypeListResponse = GetApiEventTypesResponse;
export type EventTypeResponse = GetApiEventTypesByIdResponse;
export type CreateEventTypeRequest = PostApiEventTypesData["body"];
export type UpdateEventTypeRequest = PutApiEventTypesByIdData["body"];
export type SyncEventTypesResponse =
	PostApiApplicationsByAppCodeEventTypesSyncResponse;

export interface EventTypeFilters {
	status?: string;
	application?: string;
	clientId?: string;
}

/**
 * Event Types resource for managing event type definitions.
 */
export class EventTypesResource {
	private readonly client: FlowCatalystClient;

	constructor(client: FlowCatalystClient) {
		this.client = client;
	}

	/**
	 * List all event types with optional filters.
	 */
	list(
		filters?: EventTypeFilters,
		pagination?: PaginationParams,
	): ResultAsync<EventTypeListResponse, SdkError> {
		return this.client.request<EventTypeListResponse>((httpClient, headers) =>
			sdk.getApiEventTypes({
				client: httpClient,
				headers,
				query: {
					pagination: pagination ?? {},
					...filters,
				},
			}),
		);
	}

	/**
	 * Get an event type by ID.
	 */
	get(id: string): ResultAsync<EventTypeResponse, SdkError> {
		return this.client.request<EventTypeResponse>((httpClient, headers) =>
			sdk.getApiEventTypesById({
				client: httpClient,
				headers,
				path: { id },
			}),
		);
	}

	/**
	 * Create a new event type.
	 */
	create(
		data: CreateEventTypeRequest,
	): ResultAsync<EventTypeResponse, SdkError> {
		return this.client.request<EventTypeResponse>((httpClient, headers) =>
			sdk.postApiEventTypes({
				client: httpClient,
				headers,
				body: data,
			}),
		);
	}

	/**
	 * Update an event type.
	 */
	update(
		id: string,
		data: UpdateEventTypeRequest,
	): ResultAsync<EventTypeResponse, SdkError> {
		return this.client.request<EventTypeResponse>((httpClient, headers) =>
			sdk.putApiEventTypesById({
				client: httpClient,
				headers,
				path: { id },
				body: data,
			}),
		);
	}

	/**
	 * Add a schema version to an event type.
	 */
	addSchemaVersion(
		id: string,
		schema: PostApiEventTypesByIdSchemasData["body"],
	): ResultAsync<EventTypeResponse, SdkError> {
		return this.client.request<EventTypeResponse>((httpClient, headers) =>
			sdk.postApiEventTypesByIdSchemas({
				client: httpClient,
				headers,
				path: { id },
				body: schema,
			}),
		);
	}

	/**
	 * Archive (soft-delete) an event type. The server's DELETE on this
	 * resource is a soft archive — the row is retained with status flipped
	 * to ARCHIVED. Named `archive` rather than `delete` to make the
	 * semantics visible (Rust and Laravel SDKs match).
	 */
	archive(id: string): ResultAsync<unknown, SdkError> {
		return this.client.request<unknown>((httpClient, headers) =>
			sdk.deleteApiEventTypesById({
				client: httpClient,
				headers,
				path: { id },
			}),
		);
	}

	/**
	 * Sync event types for an application.
	 *
	 * Calls `POST /api/applications/{applicationCode}/event-types/sync`.
	 */
	sync(
		applicationCode: string,
		eventTypes: PostApiApplicationsByAppCodeEventTypesSyncData["body"]["eventTypes"],
		removeUnlisted = false,
	): ResultAsync<SyncEventTypesResponse, SdkError> {
		return this.client.request<SyncEventTypesResponse>((httpClient, headers) =>
			sdk.postApiApplicationsByAppCodeEventTypesSync({
				client: httpClient,
				headers,
				path: { appCode: applicationCode },
				body: { eventTypes },
				query: { removeUnlisted },
			}),
		);
	}
}
