/**
 * Subscriptions Resource
 *
 * Manage event subscriptions for webhook delivery.
 */

import type { ResultAsync } from "neverthrow";
import type { SdkError } from "../errors.js";
import type { FlowCatalystClient } from "../client.js";
import * as sdk from "../generated/sdk.gen.js";
import type {
	ListSubscriptionsResponse,
	ListSubscriptionsData,
	GetSubscriptionResponse,
	CreateSubscriptionData,
	CreatedResponse,
	UpdateSubscriptionData,
	SyncSubscriptionsData,
	SyncSubscriptionsResponse as SyncSubscriptionsResponseType,
} from "../generated/types.gen.js";

export type SubscriptionListResponse = ListSubscriptionsResponse;
export type SubscriptionDto = GetSubscriptionResponse;
export type CreateSubscriptionRequest = CreateSubscriptionData["body"];
export type UpdateSubscriptionRequest = UpdateSubscriptionData["body"];
export type SyncSubscriptionsResponse = SyncSubscriptionsResponseType;
export type CreateSubscriptionResponse = CreatedResponse;

export interface SubscriptionFilters {
	clientId?: string;
	status?: string;
}

/**
 * Pagination params (page/size).
 *
 * @deprecated The platform's `GET /api/subscriptions` does not paginate; it
 * returns every matching subscription and ignores `page` / `size`.
 */
export type PaginationParams = {
	page?: number;
	size?: number;
};

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
				sdk.listSubscriptions({
					client: httpClient,
					headers,
					query: {
						...pagination,
						...filters,
					} as ListSubscriptionsData["query"],
				}),
		);
	}

	/**
	 * Get a subscription by ID.
	 */
	get(id: string): ResultAsync<SubscriptionDto, SdkError> {
		return this.client.request<SubscriptionDto>((httpClient, headers) =>
			sdk.getSubscription({
				client: httpClient,
				headers,
				path: { id },
			}),
		);
	}

	/**
	 * Create a new subscription. The platform answers `201 { id }`; call
	 * `get(id)` for the full entity.
	 */
	create(
		data: CreateSubscriptionRequest,
	): ResultAsync<CreateSubscriptionResponse, SdkError> {
		return this.client.request<CreateSubscriptionResponse>((httpClient, headers) =>
			sdk.createSubscription({
				client: httpClient,
				headers,
				body: data,
			}),
		);
	}

	/**
	 * Update a subscription. The platform answers `204 No Content`; call
	 * `get(id)` to read the result.
	 */
	update(
		id: string,
		data: UpdateSubscriptionRequest,
	): ResultAsync<void, SdkError> {
		return this.client
			.request<unknown>((httpClient, headers) =>
				sdk.updateSubscription({
					client: httpClient,
					headers,
					path: { id },
					body: data,
				}),
			)
			.map((): void => undefined);
	}

	/**
	 * Delete a subscription.
	 */
	delete(id: string): ResultAsync<unknown, SdkError> {
		return this.client.request<unknown>((httpClient, headers) =>
			sdk.deleteSubscription({
				client: httpClient,
				headers,
				path: { id },
			}),
		);
	}

	/**
	 * Pause a subscription. The platform answers `204 No Content`; call `get(id)`
	 * to read the result.
	 */
	pause(id: string): ResultAsync<void, SdkError> {
		return this.client
			.request<unknown>((httpClient, headers) =>
				sdk.pauseSubscription({
					client: httpClient,
					headers,
					path: { id },
				}),
			)
			.map((): void => undefined);
	}

	/**
	 * Resume a paused subscription. The platform answers `204 No Content`; call `get(id)`
	 * to read the result.
	 */
	resume(id: string): ResultAsync<void, SdkError> {
		return this.client
			.request<unknown>((httpClient, headers) =>
				sdk.resumeSubscription({
					client: httpClient,
					headers,
					path: { id },
				}),
			)
			.map((): void => undefined);
	}

	/**
	 * Sync subscriptions for an application.
	 *
	 * Calls `POST /api/applications/{applicationCode}/subscriptions/sync`.
	 */
	sync(
		applicationCode: string,
		subscriptions: SyncSubscriptionsData["body"]["subscriptions"],
		removeUnlisted = false,
	): ResultAsync<SyncSubscriptionsResponse, SdkError> {
		return this.client.request<SyncSubscriptionsResponse>(
			(httpClient, headers) =>
				sdk.syncSubscriptions({
					client: httpClient,
					headers,
					path: { appCode: applicationCode },
					body: { subscriptions },
					query: { removeUnlisted },
				}),
		);
	}
}
