/**
 * Dispatch jobs API client. No pagination — `msg_dispatch_jobs_read`
 * ingests at high rates, page navigation is meaningless. The endpoint
 * returns the most recent N rows matching the filters; configure with
 * `?size=` (default 50, max 1000).
 */

import { apiFetch } from "./client";
import type {
	DeliveryPlan as GenDeliveryPlan,
	DispatchJobFilterOptionsResponse,
	DispatchJobRead as GenDispatchJobRead,
	DispatchJobResponse as GenDispatchJobResponse,
	RequestSummary as GenRequestSummary,
	RequeueResponse,
} from "./generated";

// Response types alias the generated contract (api/openapi.lock.json) so
// `vue-tsc` fails on backend drift. Aliased under the historical names so
// pages keep their imports. The old hand-rolled row carried phantom fields
// (no `maxRetries` on the read row) and an index signature; the old
// filter-options shape ({value,label} arrays under applications/subdomains/
// aggregates) never matched the wire — the facets are plain string arrays
// under statuses/codes/clientIds/dispatchPoolIds/subscriptionIds/kinds.
export type DispatchJobRead = GenDispatchJobRead;
export type DispatchJobFilterOptions = DispatchJobFilterOptionsResponse;
/** The full job (write-side row): payload, metadata, attempts. */
export type DispatchJobDetail = GenDispatchJobResponse;
export type DispatchJobAttempt = NonNullable<GenDispatchJobResponse["attempts"]>[number];
/** What the platform sent on an attempt — see the Go RequestSummary. */
export type DeliveryRequestSummary = GenRequestSummary;
/** The "sign" action's answer: the delivery as it would go out right now. */
export type DeliveryPlan = GenDeliveryPlan;

export interface DispatchJobsListParams {
	size?: number;
	clientIds?: string[] | undefined;
	statuses?: string[] | undefined;
	applications?: string[] | undefined;
	subdomains?: string[] | undefined;
	aggregates?: string[] | undefined;
	codes?: string[] | undefined;
	source?: string | undefined;
	/** Exact message group. */
	messageGroup?: string | undefined;
	/** RFC3339 lower bound on createdAt. */
	since?: string | undefined;
	/** RFC3339 upper bound on createdAt. */
	until?: string | undefined;
	/** createdAt sort direction; server default is newest-first. */
	sort?: "createdAt.asc" | "createdAt.desc" | undefined;
}

function buildQuery(params: DispatchJobsListParams): string {
	const qp = new URLSearchParams();
	if (params.size != null) qp.set("size", String(params.size));
	if (params.clientIds?.length) qp.set("clientIds", params.clientIds.join(","));
	if (params.statuses?.length) qp.set("statuses", params.statuses.join(","));
	if (params.applications?.length) qp.set("applications", params.applications.join(","));
	if (params.subdomains?.length) qp.set("subdomains", params.subdomains.join(","));
	if (params.aggregates?.length) qp.set("aggregates", params.aggregates.join(","));
	if (params.codes?.length) qp.set("codes", params.codes.join(","));
	if (params.source) qp.set("source", params.source);
	if (params.messageGroup) qp.set("messageGroup", params.messageGroup);
	if (params.since) qp.set("since", params.since);
	if (params.until) qp.set("until", params.until);
	if (params.sort) qp.set("sort", params.sort);
	const s = qp.toString();
	return s ? `?${s}` : "";
}

export const dispatchJobsApi = {
	list(params: DispatchJobsListParams): Promise<DispatchJobRead[]> {
		return apiFetch(`/dispatch-jobs${buildQuery(params)}`);
	},
	get(id: string): Promise<DispatchJobDetail> {
		return apiFetch(`/dispatch-jobs/${encodeURIComponent(id)}`);
	},
	attempts(id: string): Promise<DispatchJobAttempt[]> {
		return apiFetch(`/dispatch-jobs/${encodeURIComponent(id)}/attempts`);
	},
	filterOptions(): Promise<DispatchJobFilterOptions> {
		return apiFetch(`/dispatch-jobs/filter-options`);
	},
	// Reset jobs to PENDING so the scheduler re-dispatches them. Returns the
	// number actually reset (tenant-scoped server-side). Used by the list
	// page's per-row retry + bulk "requeue selected".
	requeue(ids: string[]): Promise<RequeueResponse> {
		return apiFetch(`/dispatch-jobs/requeue`, {
			method: "POST",
			body: JSON.stringify({ ids }),
		});
	},
	// Dry run: which service account would sign, every header, and a real
	// signature over the real body — without delivering. Hand the timestamp,
	// signature and body to the subscriber's verify command to see whether
	// THEIR secret accepts it.
	sign(id: string): Promise<DeliveryPlan> {
		return apiFetch(`/dispatch-jobs/${encodeURIComponent(id)}/sign`, { method: "POST" });
	},
};
