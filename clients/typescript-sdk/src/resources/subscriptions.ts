/**
 * Subscriptions Resource
 *
 * Manage event subscriptions for webhook delivery.
 */

import type { ResultAsync } from "neverthrow";
import type { SdkError } from "../errors";
import type { FlowCatalystClient } from "../client";
import * as sdk from "../generated/sdk.gen";
import type {
	GetApiSubscriptionsResponse,
	GetApiSubscriptionsByIdResponse,
	PostApiSubscriptionsData,
	PutApiSubscriptionsByIdData,
	PostApiApplicationsByAppCodeSubscriptionsSyncData,
	PostApiApplicationsByAppCodeSubscriptionsSyncResponse,
} from "../generated/types.gen";

export type SubscriptionListResponse = GetApiSubscriptionsResponse;
export type SubscriptionDto = GetApiSubscriptionsByIdResponse;
export type CreateSubscriptionRequest = PostApiSubscriptionsData["body"];
export type UpdateSubscriptionRequest =
	PutApiSubscriptionsByIdData["body"];
export type SyncSubscriptionsResponse =
	PostApiApplicationsByAppCodeSubscriptionsSyncResponse;

export interface SubscriptionFilters {
	clientId?: string;
	status?: string;
}

import type { PaginationParams } from "../generated/types.gen";

/**
 * Subscriptions resource for managing event subscriptions.
 */
export class SubscriptionsResource {
	private readonly client: FlowCatalystClient;

	constructor(client: FlowCatalystClient) {
		this.client = client;
	}

	/**
	 * List all subscriptions with optional filters.
	 */
	list(
		filters?: SubscriptionFilters,
		pagination?: PaginationParams,
	): ResultAsync<SubscriptionListResponse, SdkError> {
		return this.client.request<SubscriptionListResponse>(
			(httpClient, headers) =>
				sdk.getApiSubscriptions({
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
	 * Get a subscription by ID.
	 */
	get(id: string): ResultAsync<SubscriptionDto, SdkError> {
		return this.client.request<SubscriptionDto>((httpClient, headers) =>
			sdk.getApiSubscriptionsById({
				client: httpClient,
				headers,
				path: { id },
			}),
		);
	}

	/**
	 * Create a new subscription.
	 */
	create(
		data: CreateSubscriptionRequest,
	): ResultAsync<SubscriptionDto, SdkError> {
		return this.client.request<SubscriptionDto>((httpClient, headers) =>
			sdk.postApiSubscriptions({
				client: httpClient,
				headers,
				body: data,
			}),
		);
	}

	/**
	 * Update a subscription.
	 */
	update(
		id: string,
		data: UpdateSubscriptionRequest,
	): ResultAsync<SubscriptionDto, SdkError> {
		return this.client.request<SubscriptionDto>((httpClient, headers) =>
			sdk.putApiSubscriptionsById({
				client: httpClient,
				headers,
				path: { id },
				body: data,
			}),
		);
	}

	/**
	 * Delete a subscription.
	 */
	delete(id: string): ResultAsync<unknown, SdkError> {
		return this.client.request<unknown>((httpClient, headers) =>
			sdk.deleteApiSubscriptionsById({
				client: httpClient,
				headers,
				path: { id },
			}),
		);
	}

	/**
	 * Pause a subscription.
	 */
	pause(id: string): ResultAsync<SubscriptionDto, SdkError> {
		return this.client.request<SubscriptionDto>((httpClient, headers) =>
			sdk.postApiSubscriptionsByIdPause({
				client: httpClient,
				headers,
				path: { id },
			}),
		);
	}

	/**
	 * Resume a paused subscription.
	 */
	resume(id: string): ResultAsync<SubscriptionDto, SdkError> {
		return this.client.request<SubscriptionDto>((httpClient, headers) =>
			sdk.postApiSubscriptionsByIdResume({
				client: httpClient,
				headers,
				path: { id },
			}),
		);
	}

	/**
	 * Sync subscriptions for an application.
	 *
	 * Calls `POST /api/applications/{applicationCode}/subscriptions/sync`.
	 */
	sync(
		applicationCode: string,
		subscriptions: PostApiApplicationsByAppCodeSubscriptionsSyncData["body"]["subscriptions"],
		removeUnlisted = false,
	): ResultAsync<SyncSubscriptionsResponse, SdkError> {
		return this.client.request<SyncSubscriptionsResponse>(
			(httpClient, headers) =>
				sdk.postApiApplicationsByAppCodeSubscriptionsSync({
					client: httpClient,
					headers,
					path: { appCode: applicationCode },
					body: { subscriptions },
					query: { removeUnlisted },
				}),
		);
	}
}
