/**
 * Event Types Resource
 *
 * Manage event type definitions and schemas.
 */

import type { ResultAsync } from "neverthrow";
import type { SdkError } from "../errors.js";
import type { FlowCatalystClient } from "../client.js";
import * as sdk from "../generated/sdk.gen.js";
import type {
	ListEventTypesResponse,
	GetEventTypeResponse,
	CreateEventTypeData,
	UpdateEventTypeData,
	AddEventTypeSchemaData,
	SyncEventTypesData,
	SyncEventTypesResponse as SyncEventTypesResponseType,
	ListEventTypesData,
} from "../generated/types.gen.js";

/** Pagination params (page/size). Mirrors the previous generated shape. */
export type PaginationParams = {
	page?: number;
	size?: number;
};

export type EventTypeListResponse = ListEventTypesResponse;
export type EventTypeResponse = GetEventTypeResponse;
export type CreateEventTypeRequest = CreateEventTypeData["body"];
export type UpdateEventTypeRequest = UpdateEventTypeData["body"];
export type SyncEventTypesResponse = SyncEventTypesResponseType;

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
			sdk.listEventTypes({
				client: httpClient,
				headers,
				query: {
					...pagination,
					...filters,
				} as ListEventTypesData["query"],
			}),
		);
	}

	/**
	 * Get an event type by ID.
	 */
	get(id: string): ResultAsync<EventTypeResponse, SdkError> {
		return this.client.request<EventTypeResponse>((httpClient, headers) =>
			sdk.getEventType({
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
			sdk.createEventType({
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
			sdk.updateEventType({
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
		schema: AddEventTypeSchemaData["body"],
	): ResultAsync<EventTypeResponse, SdkError> {
		return this.client.request<EventTypeResponse>((httpClient, headers) =>
			sdk.addEventTypeSchema({
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
	 * semantics visible (the Laravel SDK matches).
	 */
	archive(id: string): ResultAsync<unknown, SdkError> {
		return this.client.request<unknown>((httpClient, headers) =>
			sdk.deleteEventType({
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
		eventTypes: SyncEventTypesData["body"]["eventTypes"],
		removeUnlisted = false,
	): ResultAsync<SyncEventTypesResponse, SdkError> {
		return this.client.request<SyncEventTypesResponse>((httpClient, headers) =>
			sdk.syncEventTypes({
				client: httpClient,
				headers,
				path: { appCode: applicationCode },
				body: { eventTypes },
				query: { removeUnlisted },
			}),
		);
	}
}
