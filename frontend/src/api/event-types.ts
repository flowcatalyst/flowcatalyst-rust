// Stays hand-rolled: BFF endpoint — deliberately stripped from the OpenAPI
// spec (StripBFFPaths), so no generated types exist for it.
import { bffFetch } from "./client";
import type {
	EventType,
	SpecVersion,
	EventTypeStatus,
	SchemaType,
	SpecVersionStatus,
	EventTypeListResponse,
	FilterOptionsResponse,
	CreateEventTypeRequest,
	UpdateEventTypeRequest,
	AddSchemaRequest,
} from "@/types/bff";

// Re-export types for consumers
export type {
	EventType,
	SpecVersion,
	EventTypeStatus,
	SchemaType,
	SpecVersionStatus,
	EventTypeListResponse,
	FilterOptionsResponse,
	CreateEventTypeRequest,
	UpdateEventTypeRequest,
	AddSchemaRequest,
};

// Frontend-only filter type for API function params
export interface EventTypeFilters {
	applications?: string[];
	subdomains?: string[];
	aggregates?: string[];
	status?: EventTypeStatus;
}

export interface SyncPlatformResponse {
	created: number;
	updated: number;
	deleted: number;
	total: number;
	schemas: {
		created: number;
		updated: number;
		unchanged: number;
	};
}

// API functions - using BFF endpoints for JavaScript-safe string IDs
export const eventTypesApi = {
	list(filters: EventTypeFilters = {}): Promise<EventTypeListResponse> {
		const params = new URLSearchParams();
		filters.applications?.forEach((a) => params.append("application", a));
		filters.subdomains?.forEach((s) => params.append("subdomain", s));
		filters.aggregates?.forEach((a) => params.append("aggregate", a));
		if (filters.status) params.set("status", filters.status);

		const query = params.toString();
		return bffFetch(`/event-types${query ? `?${query}` : ""}`);
	},

	get(id: string): Promise<EventType> {
		return bffFetch(`/event-types/${id}`);
	},

	create(data: CreateEventTypeRequest): Promise<EventType> {
		return bffFetch("/event-types", {
			method: "POST",
			body: JSON.stringify(data),
		});
	},

	update(id: string, data: UpdateEventTypeRequest): Promise<void> {
		return bffFetch(`/event-types/${id}`, {
			method: "PATCH",
			body: JSON.stringify(data),
		});
	},

	delete(id: string): Promise<void> {
		return bffFetch(`/event-types/${id}`, { method: "DELETE" });
	},

	archive(id: string): Promise<EventType> {
		return bffFetch(`/event-types/${id}/archive`, { method: "POST" });
	},

	addSchema(id: string, data: AddSchemaRequest): Promise<EventType> {
		return bffFetch(`/event-types/${id}/schemas`, {
			method: "POST",
			body: JSON.stringify(data),
		});
	},

	finaliseSchema(id: string, version: string): Promise<EventType> {
		return bffFetch(`/event-types/${id}/schemas/${version}/finalise`, {
			method: "POST",
		});
	},

	deprecateSchema(id: string, version: string): Promise<EventType> {
		return bffFetch(`/event-types/${id}/schemas/${version}/deprecate`, {
			method: "POST",
		});
	},

	// Filter options
	getApplications(): Promise<FilterOptionsResponse> {
		return bffFetch("/event-types/filters/applications");
	},

	getSubdomains(applications?: string[]): Promise<FilterOptionsResponse> {
		const params = new URLSearchParams();
		applications?.forEach((a) => params.append("application", a));
		const query = params.toString();
		return bffFetch(
			`/event-types/filters/subdomains${query ? `?${query}` : ""}`,
		);
	},

	getAggregates(
		applications?: string[],
		subdomains?: string[],
	): Promise<FilterOptionsResponse> {
		const params = new URLSearchParams();
		applications?.forEach((a) => params.append("application", a));
		subdomains?.forEach((s) => params.append("subdomain", s));
		const query = params.toString();
		return bffFetch(
			`/event-types/filters/aggregates${query ? `?${query}` : ""}`,
		);
	},

	syncPlatform(): Promise<SyncPlatformResponse> {
		return bffFetch("/event-types/sync-platform", { method: "POST" });
	},
};
