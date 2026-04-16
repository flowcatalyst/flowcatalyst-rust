/**
 * Dispatch Pools Resource
 *
 * Manage dispatch pools for rate limiting and concurrency control.
 *
 * Uses direct HTTP calls since generated SDK functions are not yet available
 * (OpenAPI spec does not include /api/dispatch-pools routes). Will be
 * migrated to generated functions once the spec is updated.
 */

import type { ResultAsync } from "neverthrow";
import type { SdkError } from "../errors";
import type { FlowCatalystClient } from "../client";

export interface DispatchPoolDto {
	id: string;
	code: string;
	name: string;
	description: string | null;
	status: string;
	maxConcurrency: number;
	rateLimit: number | null;
	rateLimitWindow: number | null;
	clientId: string | null;
	applicationCode: string | null;
	createdAt: string;
	updatedAt: string;
}

export interface DispatchPoolListResponse {
	pools: DispatchPoolDto[];
	total: number;
}

export interface CreateDispatchPoolRequest {
	code: string;
	name: string;
	description?: string | null;
	maxConcurrency: number;
	rateLimit?: number | null;
	rateLimitWindow?: number | null;
	applicationCode?: string | null;
}

export interface UpdateDispatchPoolRequest {
	name?: string;
	description?: string | null;
	maxConcurrency?: number;
	rateLimit?: number | null;
	rateLimitWindow?: number | null;
}

export interface SyncDispatchPoolsResponse {
	created: number;
	updated: number;
	removed: number;
}

export interface DispatchPoolFilters {
	clientId?: string;
	status?: string;
	[key: string]: unknown;
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
		return this.client.request<DispatchPoolListResponse>(
			(httpClient, headers) =>
				httpClient.get({
					url: "/api/dispatch-pools",
					headers,
					query: filters,
				}),
		);
	}

	/**
	 * Get a dispatch pool by ID.
	 */
	get(id: string): ResultAsync<DispatchPoolDto, SdkError> {
		return this.client.request<DispatchPoolDto>((httpClient, headers) =>
			httpClient.get({
				url: "/api/dispatch-pools/{id}",
				headers,
				path: { id },
			}),
		);
	}

	/**
	 * Create a new dispatch pool.
	 */
	create(
		data: CreateDispatchPoolRequest,
	): ResultAsync<DispatchPoolDto, SdkError> {
		return this.client.request<DispatchPoolDto>((httpClient, headers) =>
			httpClient.post({
				url: "/api/dispatch-pools",
				headers: {
					...headers,
					"Content-Type": "application/json",
				},
				body: data,
			}),
		);
	}

	/**
	 * Update a dispatch pool.
	 */
	update(
		id: string,
		data: UpdateDispatchPoolRequest,
	): ResultAsync<DispatchPoolDto, SdkError> {
		return this.client.request<DispatchPoolDto>((httpClient, headers) =>
			httpClient.put({
				url: "/api/dispatch-pools/{id}",
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
	 * Delete (archive) a dispatch pool.
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
	 * Suspend a dispatch pool.
	 */
	suspend(id: string): ResultAsync<DispatchPoolDto, SdkError> {
		return this.client.request<DispatchPoolDto>((httpClient, headers) =>
			httpClient.post({
				url: "/api/dispatch-pools/{id}/suspend",
				headers,
				path: { id },
			}),
		);
	}

	/**
	 * Activate a dispatch pool.
	 */
	activate(id: string): ResultAsync<DispatchPoolDto, SdkError> {
		return this.client.request<DispatchPoolDto>((httpClient, headers) =>
			httpClient.post({
				url: "/api/dispatch-pools/{id}/activate",
				headers,
				path: { id },
			}),
		);
	}

	/**
	 * Sync dispatch pools for an application.
	 */
	sync(
		applicationCode: string,
		pools: Array<{ code: string; name: string; description?: string | null; maxConcurrency: number; rateLimit?: number | null; rateLimitWindow?: number | null }>,
		removeUnlisted = false,
	): ResultAsync<SyncDispatchPoolsResponse, SdkError> {
		return this.client.request<SyncDispatchPoolsResponse>(
			(httpClient, headers) =>
				httpClient.post({
					url: "/api/dispatch-pools/sync",
					headers: {
						...headers,
						"Content-Type": "application/json",
					},
					body: { applicationCode, pools },
					query: { removeUnlisted },
				}),
		);
	}
}
