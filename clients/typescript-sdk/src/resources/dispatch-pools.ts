/**
 * Dispatch Pools Resource
 *
 * Manage dispatch pools for rate limiting and concurrency control.
 *
 * Uses direct HTTP calls against the platform's `/api/dispatch-pools` routes.
 */

import type { ResultAsync } from "neverthrow";
import type { SdkError } from "../errors.js";
import type { FlowCatalystClient } from "../client.js";

export interface DispatchPoolDto {
	id: string;
	code: string;
	name: string;
	/** Omitted by the platform when unset. */
	description?: string | null;
	status: string;
	/** Maximum concurrent deliveries. */
	concurrency: number;
	/** Messages per minute; absent when the pool is concurrency-only. */
	rateLimit?: number | null;
	clientId?: string | null;
	clientIdentifier?: string | null;
	createdAt: string;
	updatedAt: string;
	/**
	 * @deprecated The platform returns `concurrency`; this is a copy of it,
	 * kept for callers of earlier SDK versions.
	 */
	maxConcurrency?: number;
	/** @deprecated The platform has no rate-limit window; never set. */
	rateLimitWindow?: number | null;
	/** @deprecated The platform does not return it; never set. */
	applicationCode?: string | null;
}

export interface DispatchPoolListResponse {
	pools: DispatchPoolDto[];
	total: number;
}

export interface CreateDispatchPoolRequest {
	code: string;
	name: string;
	description?: string | null;
	/** Maximum concurrent deliveries. */
	concurrency?: number;
	/** Messages per minute; omit for concurrency-only. */
	rateLimit?: number | null;
	/** Scope the pool to a client (omit for an anchor-level pool). */
	clientId?: string | null;
	/** @deprecated Use `concurrency`; sent as `concurrency` when that is unset. */
	maxConcurrency?: number;
	/** @deprecated The platform has no rate-limit window; not sent. */
	rateLimitWindow?: number | null;
	/** @deprecated The platform does not accept it; not sent. */
	applicationCode?: string | null;
}

export interface UpdateDispatchPoolRequest {
	name?: string;
	description?: string | null;
	/** Maximum concurrent deliveries. */
	concurrency?: number;
	rateLimit?: number | null;
	/** @deprecated Use `concurrency`; sent as `concurrency` when that is unset. */
	maxConcurrency?: number;
	/** @deprecated The platform has no rate-limit window; not sent. */
	rateLimitWindow?: number | null;
}

/** The body of a create: the new entity's id only. */
export interface CreatedResponse {
	id: string;
}

export interface SyncDispatchPoolsResponse {
	applicationCode?: string;
	created: number;
	updated: number;
	deleted?: number;
	syncedCodes?: string[];
	/** @deprecated Use `deleted`; this is a copy of it. */
	removed?: number;
}

export interface DispatchPoolFilters {
	clientId?: string;
	status?: string;
	[key: string]: unknown;
}

/** Map the deprecated request members onto the platform's wire shape. */
function toPoolWire(
	data: CreateDispatchPoolRequest | UpdateDispatchPoolRequest,
): Record<string, unknown> {
	const {
		maxConcurrency,
		rateLimitWindow: _rateLimitWindow,
		...rest
	} = data as CreateDispatchPoolRequest;
	const { applicationCode: _applicationCode, ...wire } = rest;
	if (wire.concurrency === undefined && maxConcurrency !== undefined) {
		return { ...wire, concurrency: maxConcurrency };
	}
	return wire;
}

/** Copy `concurrency` into the deprecated `maxConcurrency` alias. */
function withMaxConcurrency(p: DispatchPoolDto): DispatchPoolDto {
	return p.maxConcurrency === undefined && p.concurrency !== undefined
		? { ...p, maxConcurrency: p.concurrency }
		: p;
}

/**
 * Dispatch Pools resource for managing rate limiting and concurrency.
 */
export class DispatchPoolsResource {
	private readonly client: FlowCatalystClient;

	constructor(client: FlowCatalystClient) {
		this.client = client;
	}

	/**
	 * List all dispatch pools with optional filters.
	 */
	list(
		filters?: DispatchPoolFilters,
	): ResultAsync<DispatchPoolListResponse, SdkError> {
		return this.client
			.request<DispatchPoolListResponse>((httpClient, headers) =>
				httpClient.get({
					url: "/api/dispatch-pools",
					headers,
					query: filters,
				}),
			)
			.map((r) => ({ ...r, pools: (r.pools ?? []).map(withMaxConcurrency) }));
	}

	/**
	 * Get a dispatch pool by ID.
	 */
	get(id: string): ResultAsync<DispatchPoolDto, SdkError> {
		return this.client
			.request<DispatchPoolDto>((httpClient, headers) =>
				httpClient.get({
					url: "/api/dispatch-pools/{id}",
					headers,
					path: { id },
				}),
			)
			.map(withMaxConcurrency);
	}

	/**
	 * Create a new dispatch pool. The platform answers `201 { id }`; call
	 * `get(id)` for the full entity.
	 */
	create(
		data: CreateDispatchPoolRequest,
	): ResultAsync<CreatedResponse, SdkError> {
		return this.client.request<CreatedResponse>((httpClient, headers) =>
			httpClient.post({
				url: "/api/dispatch-pools",
				headers: {
					...headers,
					"Content-Type": "application/json",
				},
				body: toPoolWire(data),
			}),
		);
	}

	/**
	 * Update a dispatch pool. The platform answers `204 No Content`; call
	 * `get(id)` to read the result.
	 */
	update(
		id: string,
		data: UpdateDispatchPoolRequest,
	): ResultAsync<void, SdkError> {
		return this.client
			.request<unknown>((httpClient, headers) =>
				httpClient.put({
					url: "/api/dispatch-pools/{id}",
					headers: {
						...headers,
						"Content-Type": "application/json",
					},
					path: { id },
					body: toPoolWire(data),
				}),
			)
			.map((): void => undefined);
	}

	/**
	 * Archive a dispatch pool (soft-delete): `POST
	 * /api/dispatch-pools/{id}/archive`, answered `204 No Content`.
	 */
	archive(id: string): ResultAsync<void, SdkError> {
		return this.voidPost("/api/dispatch-pools/{id}/archive", id);
	}

	/**
	 * Delete a dispatch pool: `DELETE /api/dispatch-pools/{id}` removes the
	 * row. Use `archive(id)` for a soft-delete.
	 */
	delete(id: string): ResultAsync<unknown, SdkError> {
		return this.client.request<unknown>((httpClient, headers) =>
			httpClient.delete({
				url: "/api/dispatch-pools/{id}",
				headers,
				path: { id },
			}),
		);
	}

	/**
	 * Suspend a dispatch pool. The platform answers `204 No Content`; call
	 * `get(id)` to read the result.
	 */
	suspend(id: string): ResultAsync<void, SdkError> {
		return this.voidPost("/api/dispatch-pools/{id}/suspend", id);
	}

	/**
	 * Activate a dispatch pool. The platform answers `204 No Content`; call
	 * `get(id)` to read the result.
	 */
	activate(id: string): ResultAsync<void, SdkError> {
		return this.voidPost("/api/dispatch-pools/{id}/activate", id);
	}

	/**
	 * Sync dispatch pools for an application.
	 *
	 * Calls `POST /api/applications/{applicationCode}/dispatch-pools/sync`.
	 * The platform accepts only `code`, `name`, `description`,
	 * `concurrency` and `rateLimit` per pool; a `null` member is left out.
	 */
	sync(
		applicationCode: string,
		pools: Array<{ code: string; name: string; description?: string | null; concurrency: number; rateLimit?: number | null }>,
		removeUnlisted = false,
	): ResultAsync<SyncDispatchPoolsResponse, SdkError> {
		const wire = pools.map((p) => ({
			code: p.code,
			name: p.name,
			...(p.description != null ? { description: p.description } : {}),
			...(p.concurrency != null ? { concurrency: p.concurrency } : {}),
			...(p.rateLimit != null ? { rateLimit: p.rateLimit } : {}),
		}));
		return this.client
			.request<SyncDispatchPoolsResponse>((httpClient, headers) =>
				httpClient.post({
					url: `/api/applications/${encodeURIComponent(applicationCode)}/dispatch-pools/sync`,
					headers: {
						...headers,
						"Content-Type": "application/json",
					},
					body: { pools: wire },
					query: { removeUnlisted },
				}),
			)
			.map((r) =>
				r.removed === undefined && r.deleted !== undefined
					? { ...r, removed: r.deleted }
					: r,
			);
	}

	private voidPost(url: string, id: string): ResultAsync<void, SdkError> {
		return this.client
			.request<unknown>((httpClient, headers) =>
				httpClient.post({
					url,
					headers,
					path: { id },
				}),
			)
			.map((): void => undefined);
	}
}
