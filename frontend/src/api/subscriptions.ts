import { apiFetch } from "./client";
import type {
	ConfigEntryDto,
	CreatedResponse,
	EventTypeBindingDto,
	SubscriptionListResponse as GenSubscriptionListResponse,
	SubscriptionResponse,
} from "./generated";

// Request-side string unions the forms rely on. The generated response
// types deliberately stay `string` (the spec doesn't carry enums — see
// docs/frontend-api-types-adoption.md on SDK coordination).
export type SubscriptionStatus = "ACTIVE" | "PAUSED";
export type SubscriptionSource = "API" | "UI";
export type SubscriptionMode = "IMMEDIATE" | "NEXT_ON_ERROR" | "BLOCK_ON_ERROR";

// Response types alias the generated contract (api/openapi.lock.json) so
// `vue-tsc` fails on backend drift. Aliased under the historical names so
// pages keep their imports.
export type Subscription = SubscriptionResponse;
export type SubscriptionListResponse = GenSubscriptionListResponse;
export type EventTypeBinding = EventTypeBindingDto;
export type ConfigEntry = ConfigEntryDto;

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

	/** POST /subscriptions returns the standard created envelope `{ id }`, not the full subscription. */
	create(data: CreateSubscriptionRequest): Promise<CreatedResponse> {
		return apiFetch("/subscriptions", {
			method: "POST",
			body: JSON.stringify(data),
		});
	},

	update(id: string, data: UpdateSubscriptionRequest): Promise<void> {
		return apiFetch(`/subscriptions/${id}`, {
			method: "PUT",
			body: JSON.stringify(data),
		});
	},

	// delete/pause/resume return 204 No Content on the wire; the old
	// `{ message, subscriptionId }` envelope never existed.
	delete(id: string): Promise<void> {
		return apiFetch(`/subscriptions/${id}`, {
			method: "DELETE",
		});
	},

	pause(id: string): Promise<void> {
		return apiFetch(`/subscriptions/${id}/pause`, {
			method: "POST",
		});
	},

	resume(id: string): Promise<void> {
		return apiFetch(`/subscriptions/${id}/resume`, {
			method: "POST",
		});
	},
};
