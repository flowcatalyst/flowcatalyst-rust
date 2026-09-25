/**
 * Scheduled Jobs Resource — CRUD + state transitions + history reads.
 *
 * Hand-typed (does not depend on generated OpenAPI types) so it works
 * without regenerating the SDK after server-side endpoint changes. Mirrors
 * the shape of `EventTypesResource` for callers.
 *
 * SDK callbacks (`logForInstance`, `completeInstance`) live here too so
 * consumers don't need a second resource accessor.
 */

import { ResultAsync, errAsync, okAsync } from "neverthrow";
import type { SdkError } from "../errors.js";
import { httpError, mapHttpStatusToError } from "../errors.js";
import type { FlowCatalystClient } from "../client.js";

// ── Domain types (kept intentionally close to the platform DTOs) ────────────

export type ScheduledJobStatus = "ACTIVE" | "PAUSED" | "ARCHIVED";
export type TriggerKind = "CRON" | "MANUAL";
export type InstanceStatus =
	| "QUEUED"
	| "IN_FLIGHT"
	| "DELIVERED"
	| "COMPLETED"
	| "FAILED"
	| "DELIVERY_FAILED";
export type CompletionStatus = "SUCCESS" | "FAILURE";
export type LogLevel = "DEBUG" | "INFO" | "WARN" | "ERROR";

export interface ScheduledJob {
	id: string;
	clientId?: string | null;
	code: string;
	name: string;
	description?: string;
	status: ScheduledJobStatus;
	crons: string[];
	timezone: string;
	payload?: unknown;
	concurrent: boolean;
	tracksCompletion: boolean;
	timeoutSeconds?: number;
	deliveryMaxAttempts: number;
	targetUrl?: string;
	lastFiredAt?: string;
	createdAt: string;
	updatedAt: string;
	createdBy?: string;
	updatedBy?: string;
	version: number;
	hasActiveInstance: boolean;
}

export interface ScheduledJobInstance {
	id: string;
	scheduledJobId: string;
	clientId?: string | null;
	jobCode: string;
	triggerKind: TriggerKind;
	scheduledFor?: string;
	firedAt: string;
	deliveredAt?: string;
	completedAt?: string;
	status: InstanceStatus;
	deliveryAttempts: number;
	deliveryError?: string;
	completionStatus?: CompletionStatus;
	completionResult?: unknown;
	correlationId?: string;
	createdAt: string;
}

export interface ScheduledJobInstanceLog {
	id: string;
	instanceId: string;
	level: LogLevel;
	message: string;
	metadata?: unknown;
	createdAt: string;
}

export interface PaginatedJobs {
	data: ScheduledJob[];
	page: number;
	size: number;
	total: number;
	totalPages: number;
}

export interface PaginatedInstances {
	data: ScheduledJobInstance[];
	page: number;
	size: number;
	total: number;
	totalPages: number;
}

export interface CreateScheduledJobRequest {
	code: string;
	name: string;
	description?: string;
	clientId?: string | null;
	/** The registered Application that owns this job — a separate axis from clientId (tenant vs. registered app). */
	applicationId?: string | null;
	crons: string[];
	timezone?: string;
	payload?: unknown;
	concurrent?: boolean;
	tracksCompletion?: boolean;
	timeoutSeconds?: number;
	deliveryMaxAttempts?: number;
	targetUrl?: string;
}

export interface UpdateScheduledJobRequest {
	name?: string;
	description?: string;
	crons?: string[];
	timezone?: string;
	payload?: unknown;
	concurrent?: boolean;
	tracksCompletion?: boolean;
	timeoutSeconds?: number;
	deliveryMaxAttempts?: number;
	targetUrl?: string;
}

export interface ListJobsFilters {
	clientId?: string | "platform";
	status?: ScheduledJobStatus;
	search?: string;
	page?: number;
	size?: number;
}

export interface ListInstancesFilters {
	status?: InstanceStatus;
	triggerKind?: TriggerKind;
	from?: string;
	to?: string;
	page?: number;
	size?: number;
}

export interface FireRequest {
	correlationId?: string;
}

export interface InstanceLogRequest {
	message: string;
	level?: LogLevel;
	metadata?: unknown;
}

export interface InstanceCompleteRequest {
	status: CompletionStatus;
	result?: unknown;
}

// ── Resource ────────────────────────────────────────────────────────────────

const PATH = "/api/scheduled-jobs";

export class ScheduledJobsResource {
	private readonly client: FlowCatalystClient;

	constructor(client: FlowCatalystClient) {
		this.client = client;
	}

	/** Create a new scheduled job. Returns the new job's id. */
	create(req: CreateScheduledJobRequest): ResultAsync<{ id: string }, SdkError> {
		return this.fetch<{ id: string }>("POST", PATH, req);
	}

	list(filters: ListJobsFilters = {}): ResultAsync<PaginatedJobs, SdkError> {
		return this.fetch<PaginatedJobs>(
			"GET",
			`${PATH}${qs(filters as Record<string, unknown>)}`,
		);
	}

	get(id: string): ResultAsync<ScheduledJob, SdkError> {
		return this.fetch<ScheduledJob>("GET", `${PATH}/${encodeURIComponent(id)}`);
	}

	getByCode(
		code: string,
		clientId?: string,
	): ResultAsync<ScheduledJob, SdkError> {
		const q = clientId ? qs({ clientId }) : "";
		return this.fetch<ScheduledJob>(
			"GET",
			`${PATH}/by-code/${encodeURIComponent(code)}${q}`,
		);
	}

	update(id: string, req: UpdateScheduledJobRequest): ResultAsync<void, SdkError> {
		return this.fetch<void>("PUT", `${PATH}/${encodeURIComponent(id)}`, req);
	}

	pause(id: string): ResultAsync<void, SdkError> {
		return this.fetch<void>("POST", `${PATH}/${encodeURIComponent(id)}/pause`);
	}

	resume(id: string): ResultAsync<void, SdkError> {
		return this.fetch<void>("POST", `${PATH}/${encodeURIComponent(id)}/resume`);
	}

	archive(id: string): ResultAsync<void, SdkError> {
		return this.fetch<void>("POST", `${PATH}/${encodeURIComponent(id)}/archive`);
	}

	delete(id: string): ResultAsync<void, SdkError> {
		return this.fetch<void>("DELETE", `${PATH}/${encodeURIComponent(id)}`);
	}

	/** Manually fire a scheduled job. Returns the new instance's id. */
	fire(id: string, req: FireRequest = {}): ResultAsync<{ id: string }, SdkError> {
		return this.fetch<{ id: string }>(
			"POST",
			`${PATH}/${encodeURIComponent(id)}/fire`,
			req,
		);
	}

	listInstances(
		jobId: string,
		filters: ListInstancesFilters = {},
	): ResultAsync<PaginatedInstances, SdkError> {
		return this.fetch<PaginatedInstances>(
			"GET",
			`${PATH}/${encodeURIComponent(jobId)}/instances${qs(filters as Record<string, unknown>)}`,
		);
	}

	getInstance(instanceId: string): ResultAsync<ScheduledJobInstance, SdkError> {
		return this.fetch<ScheduledJobInstance>(
			"GET",
			`${PATH}/instances/${encodeURIComponent(instanceId)}`,
		);
	}

	listInstanceLogs(
		instanceId: string,
	): ResultAsync<ScheduledJobInstanceLog[], SdkError> {
		return this.fetch<ScheduledJobInstanceLog[]>(
			"GET",
			`${PATH}/instances/${encodeURIComponent(instanceId)}/logs`,
		);
	}

	// ── SDK callback paths (used by the runner; safe to call directly too) ──

	logForInstance(
		instanceId: string,
		req: InstanceLogRequest,
	): ResultAsync<void, SdkError> {
		return this.fetch<void>(
			"POST",
			`${PATH}/instances/${encodeURIComponent(instanceId)}/log`,
			req,
		);
	}

	completeInstance(
		instanceId: string,
		req: InstanceCompleteRequest,
	): ResultAsync<void, SdkError> {
		return this.fetch<void>(
			"POST",
			`${PATH}/instances/${encodeURIComponent(instanceId)}/complete`,
			req,
		);
	}

	// ── Internal: thin fetch wrapper that reuses the client's auth + retry. ──

	private fetch<T>(
		method: string,
		path: string,
		body?: unknown,
	): ResultAsync<T, SdkError> {
		// Reuse the client's `request()` retry/auth machinery by passing a
		// fn that ignores the generated `Client` arg and does its own fetch.
		return this.client.request<T>(async (_genClient, headers) => {
			const url = (this.client as unknown as { config: { baseUrl: string } })
				.config.baseUrl + path;
			const init: RequestInit = {
				method,
				headers: {
					...headers,
					...(body !== undefined ? { "Content-Type": "application/json" } : {}),
				},
				body: body !== undefined ? JSON.stringify(body) : undefined,
			};
			const response = await fetch(url, init);
			if (!response.ok) {
				let errorBody: unknown = undefined;
				try {
					errorBody = await response.json();
				} catch {
					/* response had no JSON body */
				}
				return { error: errorBody, response };
			}
			// 204 No Content → resolve to undefined
			if (response.status === 204) {
				return { data: undefined as unknown as T, response };
			}
			const data = await response.json();
			return { data, response };
		});
	}
}

/** Build a `?key=value&...` querystring from a flat object. Skips nullish. */
function qs(obj: Record<string, unknown>): string {
	const entries = Object.entries(obj).filter(
		([, v]) => v !== undefined && v !== null && v !== "",
	);
	if (entries.length === 0) return "";
	const sp = new URLSearchParams();
	for (const [k, v] of entries) sp.append(k, String(v));
	return `?${sp.toString()}`;
}

// Export a tiny helper for users who want to build URL-aware error helpers.
export { mapHttpStatusToError, httpError, ResultAsync, errAsync, okAsync };
