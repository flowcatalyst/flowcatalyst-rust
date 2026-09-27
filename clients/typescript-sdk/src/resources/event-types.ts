/**
 * Event Types Resource
 *
 * Manage event type definitions and schemas.
 */

import { okAsync, type ResultAsync } from "neverthrow";
import type { SdkError } from "../errors.js";
import type { FlowCatalystClient } from "../client.js";
import * as sdk from "../generated/sdk.gen.js";
import type {
	ListEventTypesResponse,
	GetEventTypeResponse,
	CreateEventTypeData,
	CreatedResponse,
	UpdateEventTypeData,
	AddEventTypeSchemaData,
	SyncEventTypesData,
	SyncEventTypesResponse as SyncEventTypesResponseType,
	ListEventTypesData,
} from "../generated/types.gen.js";

/**
 * Pagination params (page/size).
 *
 * @deprecated The platform's `GET /api/event-types` does not paginate; it
 * returns every matching event type and ignores `page` / `size`.
 */
export type PaginationParams = {
	page?: number;
	size?: number;
};

export type EventTypeListResponse = ListEventTypesResponse;
export type EventTypeResponse = GetEventTypeResponse;
export type CreateEventTypeRequest = CreateEventTypeData["body"];
export type UpdateEventTypeRequest = UpdateEventTypeData["body"];
export type SyncEventTypesResponse = SyncEventTypesResponseType;
export type CreateEventTypeResponse = CreatedResponse;

export interface EventTypeFilters {
	status?: string;
	application?: string;
	clientId?: string;
	subdomain?: string;
	aggregate?: string;
}

type SyncEventTypeInput = SyncEventTypesData["body"]["eventTypes"][number];

/**
 * Keep only the members the platform's strict `SyncEventTypeInputRequest`
 * accepts (`code`, `name`, `description`). It rejects any other member, so a
 * caller's extra fields (e.g. `schema`, `clientId`) would fail the whole sync.
 */
export function toSyncEventTypeInput(e: {
	code: string;
	name: string;
	description?: string;
}): SyncEventTypeInput {
	return {
		code: e.code,
		name: e.name,
		...(e.description !== undefined ? { description: e.description } : {}),
	};
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
	 * List event types with optional filters. With no filter the platform
	 * returns `CURRENT` event types only.
	 *
	 * `pagination` is ignored by the platform (see `PaginationParams`).
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
	 * Get an event type by code (`{app}:{subdomain}:{aggregate}:{event}`).
	 */
	getByCode(code: string): ResultAsync<EventTypeResponse, SdkError> {
		return this.client.request<EventTypeResponse>((httpClient, headers) =>
			sdk.getEventTypeByCode({
				client: httpClient,
				headers,
				path: { code },
			}),
		);
	}

	/**
	 * Create a new event type. The platform answers `201 { id }`; call
	 * `get(id)` for the full entity.
	 */
	create(
		data: CreateEventTypeRequest,
	): ResultAsync<CreateEventTypeResponse, SdkError> {
		return this.client.request<CreateEventTypeResponse>((httpClient, headers) =>
			sdk.createEventType({
				client: httpClient,
				headers,
				body: data,
			}),
		);
	}

	/**
	 * Update an event type. The platform answers `204 No Content`; call
	 * `get(id)` to read the result.
	 *
	 * The platform treats the body as a replacement: `name` is required and
	 * an omitted `description` clears it. When `name` is missing (an untyped
	 * caller), the current name is read first and sent unchanged.
	 */
	update(
		id: string,
		data: UpdateEventTypeRequest,
	): ResultAsync<void, SdkError> {
		const body: ResultAsync<UpdateEventTypeRequest, SdkError> =
			data.name !== undefined && data.name !== null
				? okAsync(data)
				: this.get(id).map((current) => ({ ...data, name: current.name }));
		return body
			.andThen((b) =>
				this.client.request<unknown>((httpClient, headers) =>
					sdk.updateEventType({
						client: httpClient,
						headers,
						path: { id },
						body: b,
					}),
				),
			)
			.map((): void => undefined);
	}

	/**
	 * Add a schema version to an event type. Returns the updated event type.
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
	 * Delete an event type (`DELETE /api/event-types/{id}`). The row is
	 * removed; the platform has no separate archive route for event types.
	 */
	delete(id: string): ResultAsync<void, SdkError> {
		return this.client
			.request<unknown>((httpClient, headers) =>
				sdk.deleteEventType({
					client: httpClient,
					headers,
					path: { id },
				}),
			)
			.map((): void => undefined);
	}

	/**
	 * @deprecated The platform has no archive route for event types. This
	 * sends `DELETE /api/event-types/{id}`, which removes the event type (it
	 * is not a soft archive). Use `delete(id)`, which does the same, so the
	 * call site says what happens.
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
	 * Each entry is reduced to `code`, `name` and `description`, the only
	 * members the platform accepts; schemas are added with
	 * `addSchemaVersion`.
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
				body: { eventTypes: eventTypes.map(toSyncEventTypeInput) },
				query: { removeUnlisted },
			}),
		);
	}
}
