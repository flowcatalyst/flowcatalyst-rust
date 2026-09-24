/**
 * API client for Audit Log operations.
 */

import { apiFetch, bffFetch } from "./client";

export interface AuditLog {
	id: string;
	entityType: string;
	entityId: string;
	operation: string;
	principalId: string | null;
	principalName: string | null;
	applicationId: string | null;
	clientId: string | null;
	performedAt: string;
}

export interface AuditLogDetail extends AuditLog {
	operationJson: string | null;
}

/**
 * Cursor-paginated audit logs response. Backend keysets on
 * `(performedAt, id) DESC` and never counts — `aud_logs` is unbounded.
 */
export interface AuditLogListResponse {
	auditLogs: AuditLog[];
	hasMore: boolean;
	nextCursor?: string;
}

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
export async function fetchEntityTypes(): Promise<{ entityTypes: string[] }> {
	return apiFetch<{ entityTypes: string[] }>("/audit-logs/entity-types");
}

/**
 * Fetch distinct operations that have audit logs.
 */
export async function fetchOperations(): Promise<{ operations: string[] }> {
	return apiFetch<{ operations: string[] }>("/audit-logs/operations");
}

/**
 * Fetch distinct application IDs present in audit logs.
 */
export async function fetchDistinctApplicationIds(): Promise<{ applicationIds: string[] }> {
	return apiFetch<{ applicationIds: string[] }>("/audit-logs/application-ids");
}

/**
 * Fetch distinct client IDs present in audit logs.
 */
export async function fetchDistinctClientIds(): Promise<{ clientIds: string[] }> {
	return apiFetch<{ clientIds: string[] }>("/audit-logs/client-ids");
}

/**
 * TEMPORARY (docs/spec/audit-redaction.md in the Java repo, "Temporary:
 * redact existing rows from the dashboard"; remove with the dashboard card):
 * redacts passwords and secrets already stored in `aud_logs.operation_json`
 * by rows written before the source-side redaction. Anchor-only BFF route
 * (the rest of this file reads `/api/audit-logs`). Mirrors the backend's
 * `RedactExistingAuditLogsResponse`: candidate rows read, rows rewritten.
 */
export interface RedactExistingAuditLogsResponse {
	scanned: number;
	redacted: number;
}

/** See {@link RedactExistingAuditLogsResponse}. */
export async function redactExistingAuditLogs(): Promise<RedactExistingAuditLogsResponse> {
	return bffFetch<RedactExistingAuditLogsResponse>("/audit-logs/redact-existing", {
		method: "POST",
	});
}
