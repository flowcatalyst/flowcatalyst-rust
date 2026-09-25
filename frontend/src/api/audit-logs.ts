/**
 * API client for Audit Log operations.
 */

import { apiFetch } from "./client";
import type {
	AuditLogApplicationIdsResponse,
	AuditLogClientIdsResponse,
	AuditLogEntityTypesResponse,
	AuditLogListResponse as GenAuditLogListResponse,
	AuditLogOperationsResponse,
	AuditLogResponse,
} from "./generated";

// Response types alias the generated contract (api/openapi.lock.json) so
// `vue-tsc` fails on backend drift. Aliased under the historical names so
// pages keep their imports. Absent fields are `undefined` (not `null`) on
// the wire, and the detail endpoint returns the same AuditLogResponse —
// `operationJson` lives on the base shape (optional).
export type AuditLog = AuditLogResponse;
export type AuditLogDetail = AuditLogResponse;

/**
 * Cursor-paginated audit logs response. Backend keysets on
 * `(performedAt, id) DESC` and never counts — `aud_logs` is unbounded.
 */
export type AuditLogListResponse = GenAuditLogListResponse;

export interface AuditLogFilters {
	entityType?: string;
	entityId?: string;
	principalId?: string;
	operation?: string;
	applicationIds?: string[];
	clientIds?: string[];
	/** Opaque cursor returned by a previous response. Omit for the first page. */
	after?: string | undefined;
	pageSize?: number;
}

/**
 * Fetch a page of audit logs (cursor-paginated).
 */
export async function fetchAuditLogs(
	filters: AuditLogFilters = {},
): Promise<AuditLogListResponse> {
	const params = new URLSearchParams();
	if (filters.entityType) params.set("entityType", filters.entityType);
	if (filters.entityId) params.set("entityId", filters.entityId);
	if (filters.principalId) params.set("principalId", filters.principalId);
	if (filters.operation) params.set("operation", filters.operation);
	if (filters.applicationIds?.length) params.set("applicationIds", filters.applicationIds.join(","));
	if (filters.clientIds?.length) params.set("clientIds", filters.clientIds.join(","));
	if (filters.after) params.set("after", filters.after);
	if (filters.pageSize !== undefined)
		params.set("pageSize", String(filters.pageSize));

	const query = params.toString();
	return apiFetch<AuditLogListResponse>(
		`/audit-logs${query ? `?${query}` : ""}`,
	);
}

/**
 * Fetch a single audit log by ID.
 */
export async function fetchAuditLogById(id: string): Promise<AuditLogDetail> {
	return apiFetch<AuditLogDetail>(`/audit-logs/${id}`);
}

/**
 * Fetch audit logs for a specific entity.
 */
export async function fetchAuditLogsForEntity(
	entityType: string,
	entityId: string,
): Promise<AuditLogListResponse> {
	return apiFetch<AuditLogListResponse>(
		`/audit-logs/entity/${encodeURIComponent(entityType)}/${encodeURIComponent(entityId)}`,
	);
}

/**
 * Fetch distinct entity types that have audit logs.
 */
export async function fetchEntityTypes(): Promise<AuditLogEntityTypesResponse> {
	return apiFetch<AuditLogEntityTypesResponse>("/audit-logs/entity-types");
}

/**
 * Fetch distinct operations that have audit logs.
 */
export async function fetchOperations(): Promise<AuditLogOperationsResponse> {
	return apiFetch<AuditLogOperationsResponse>("/audit-logs/operations");
}

/**
 * Fetch distinct application IDs present in audit logs.
 */
export async function fetchDistinctApplicationIds(): Promise<AuditLogApplicationIdsResponse> {
	return apiFetch<AuditLogApplicationIdsResponse>("/audit-logs/application-ids");
}

/**
 * Fetch distinct client IDs present in audit logs.
 */
export async function fetchDistinctClientIds(): Promise<AuditLogClientIdsResponse> {
	return apiFetch<AuditLogClientIdsResponse>("/audit-logs/client-ids");
}
