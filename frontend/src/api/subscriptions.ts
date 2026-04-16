import { apiFetch } from "./client";

export type SubscriptionStatus = "ACTIVE" | "PAUSED";
export type SubscriptionSource = "API" | "UI";
export type SubscriptionMode = "IMMEDIATE" | "NEXT_ON_ERROR" | "BLOCK_ON_ERROR";

export interface EventTypeBinding {
	eventTypeId: string;
	eventTypeCode: string;
	specVersion: string;
}

export interface ConfigEntry {
	key: string;
	value: string;
}

export interface Subscription {
	id: string;
	code: string;
	applicationCode?: string;
	name: string;
	description?: string;
	endpoint: string;
	clientScoped: boolean;
	clientId?: string;
	clientIdentifier?: string;
	eventTypes: EventTypeBinding[];
	connectionId?: string;
	queue: string;
	customConfig?: ConfigEntry[];
	source: SubscriptionSource;
	status: SubscriptionStatus;
	maxAgeSeconds: number;
	dispatchPoolId: string;
	dispatchPoolCode: string;
	delaySeconds: number;
	sequence: number;
	mode: SubscriptionMode;
	timeoutSeconds: number;
	createdAt: string;
	updatedAt: string;
}

export interface SubscriptionListResponse {
	subscriptions: Subscription[];
	total: number;
}

export interface CreateSubscriptionRequest {
	code: string;
	applicationCode?: string;
	name: string;
	description?: string;
	endpoint: string;
	clientScoped: boolean;
	clientId?: string;
	eventTypes: EventTypeBinding[];
	connectionId?: string;
	queue: string;
	customConfig?: ConfigEntry[];
	source?: SubscriptionSource;
	maxAgeSeconds?: number;
	dispatchPoolId: string;
	delaySeconds?: number;
	sequence?: number;
	mode?: SubscriptionMode;
	timeoutSeconds?: number;
}

export interface UpdateSubscriptionRequest {
	name?: string;
	description?: string;
	endpoint?: string;
	eventTypes?: EventTypeBinding[];
	connectionId?: string;
	queue?: string;
	customConfig?: ConfigEntry[];
	status?: SubscriptionStatus;
	maxAgeSeconds?: number;
	dispatchPoolId?: string;
	delaySeconds?: number;
	sequence?: number;
	mode?: SubscriptionMode;
	timeoutSeconds?: number;
}

export interface SubscriptionFilters {
	clientId?: string;
	status?: SubscriptionStatus;
	source?: SubscriptionSource;
	dispatchPoolId?: string;
	applicationCode?: string;
	anchorLevel?: boolean;
}

export interface StatusResponse {
	message: string;
	subscriptionId: string;
}

export const subscriptionsApi = {
	list(filters: SubscriptionFilters = {}): Promise<SubscriptionListResponse> {
		const params = new URLSearchParams();
		if (filters.clientId) params.set("clientId", filters.clientId);
		if (filters.status) params.set("status", filters.status);
		if (filters.source) params.set("source", filters.source);
		if (filters.dispatchPoolId)
			params.set("dispatchPoolId", filters.dispatchPoolId);
		if (filters.applicationCode)
			params.set("applicationCode", filters.applicationCode);
		if (filters.anchorLevel !== undefined)
			params.set("anchorLevel", String(filters.anchorLevel));

		const query = params.toString();
		return apiFetch(`/subscriptions${query ? `?${query}` : ""}`);
	},

	get(id: string): Promise<Subscription> {
		return apiFetch(`/subscriptions/${id}`);
	},

	create(data: CreateSubscriptionRequest): Promise<Subscription> {
		return apiFetch("/subscriptions", {
			method: "POST",
			body: JSON.stringify(data),
		});
	},

	update(id: string, data: UpdateSubscriptionRequest): Promise<Subscription> {
		return apiFetch(`/subscriptions/${id}`, {
			method: "PUT",
			body: JSON.stringify(data),
		});
	},

	delete(id: string): Promise<StatusResponse> {
		return apiFetch(`/subscriptions/${id}`, {
			method: "DELETE",
		});
	},

	pause(id: string): Promise<StatusResponse> {
		return apiFetch(`/subscriptions/${id}/pause`, {
			method: "POST",
		});
	},

	resume(id: string): Promise<StatusResponse> {
		return apiFetch(`/subscriptions/${id}/resume`, {
			method: "POST",
		});
	},
};
