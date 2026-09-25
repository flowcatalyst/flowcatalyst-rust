/**
 * Events API client. No pagination — `msg_events_read` ingests at high
 * rates, page navigation is meaningless. The endpoint returns the most
 * recent N rows matching the filters; configure with `?size=` (default
 * 50, max 1000).
 */

import { apiFetch } from "./client";
import type {
	EventFilterOptionsResponse,
	EventRead as GenEventRead,
	EventResponse,
} from "./generated";

// Response types alias the generated contract (api/openapi.lock.json) so
// `vue-tsc` fails on backend drift. Aliased under the historical names so
// pages keep their imports. Absent fields are `undefined` (not `null`) on
// the wire; GET /events/{id} returns the full EventResponse, which is not
// a strict superset of the list row (e.g. `projectedAt` is optional there).
export type EventRead = GenEventRead;
export type EventDetail = EventResponse;
export type EventFilterOptions = EventFilterOptionsResponse;

export interface EventsListParams {
	size?: number;
	clientIds?: string[] | undefined;
	applications?: string[] | undefined;
	subdomains?: string[] | undefined;
	aggregates?: string[] | undefined;
	types?: string[] | undefined;
	correlationId?: string | undefined;
	source?: string | undefined;
}

function buildQuery(params: EventsListParams): string {
	const qp = new URLSearchParams();
	if (params.size != null) qp.set("size", String(params.size));
	if (params.clientIds?.length) qp.set("clientIds", params.clientIds.join(","));
	if (params.applications?.length) qp.set("applications", params.applications.join(","));
	if (params.subdomains?.length) qp.set("subdomains", params.subdomains.join(","));
	if (params.aggregates?.length) qp.set("aggregates", params.aggregates.join(","));
	if (params.types?.length) qp.set("types", params.types.join(","));
	if (params.correlationId) qp.set("correlationId", params.correlationId);
	if (params.source) qp.set("source", params.source);
	const s = qp.toString();
	return s ? `?${s}` : "";
}

export const eventsApi = {
	list(params: EventsListParams): Promise<EventRead[]> {
		return apiFetch(`/events${buildQuery(params)}`);
	},
	get(id: string): Promise<EventDetail> {
		return apiFetch(`/events/${encodeURIComponent(id)}`);
	},
	filterOptions(): Promise<EventFilterOptions> {
		return apiFetch(`/events/filter-options`);
	},
};
