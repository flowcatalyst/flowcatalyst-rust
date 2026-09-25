/**
 * Audit Logs Resource
 *
 * Query the platform's `iam_audit_logs` table — every UoW commit emits a
 * row here in addition to its domain event. Read-only by design.
 */

import type { ResultAsync } from "neverthrow";
import type { SdkError } from "../errors.js";
import type { FlowCatalystClient } from "../client.js";
import * as sdk from "../generated/sdk.gen.js";
import type {
	ListAuditLogsData,
	ListAuditLogsResponse,
	GetAuditLogResponse,
	ListAuditLogsRecentResponse,
	AuditLogsByEntityResponse,
	AuditLogsByPrincipalResponse,
} from "../generated/types.gen.js";

export type AuditLogFilters = ListAuditLogsData["query"];
export type AuditLogListResponse = ListAuditLogsResponse;
export type AuditLogDto = GetAuditLogResponse;
export type RecentAuditLogsResponse = ListAuditLogsRecentResponse;
export type AuditLogsForEntityResponse = AuditLogsByEntityResponse;
export type AuditLogsForPrincipalResponse = AuditLogsByPrincipalResponse;

/**
 * Audit logs resource for querying audit history.
 */
export class AuditLogsResource {
	private readonly client: FlowCatalystClient;

	constructor(client: FlowCatalystClient) {
		this.client = client;
	}

	/**
	 * List audit logs with optional filters and pagination.
	 */
	list(
		filters?: AuditLogFilters,
	): ResultAsync<AuditLogListResponse, SdkError> {
		return this.client.request<AuditLogListResponse>((httpClient, headers) =>
			sdk.listAuditLogs({
				client: httpClient,
				headers,
				query: filters,
			}),
		);
	}

	/**
	 * Get a single audit log entry by ID.
	 */
	get(id: string): ResultAsync<AuditLogDto, SdkError> {
		return this.client.request<AuditLogDto>((httpClient, headers) =>
			sdk.getAuditLog({
				client: httpClient,
				headers,
				path: { id },
			}),
		);
	}

	/**
	 * Fetch recent audit log entries (typically last 100, server-defined).
	 */
	recent(): ResultAsync<RecentAuditLogsResponse, SdkError> {
		return this.client.request<RecentAuditLogsResponse>(
			(httpClient, headers) =>
				sdk.listAuditLogsRecent({
					client: httpClient,
					headers,
				}),
		);
	}

	/**
	 * Fetch audit log entries for a specific entity.
	 */
	forEntity(
		entityType: string,
		entityId: string,
	): ResultAsync<AuditLogsForEntityResponse, SdkError> {
		return this.client.request<AuditLogsForEntityResponse>(
			(httpClient, headers) =>
				sdk.auditLogsByEntity({
					client: httpClient,
					headers,
					path: { entityType, entityId },
				}),
		);
	}

	/**
	 * Fetch audit log entries for actions performed by a specific principal.
	 */
	forPrincipal(
		principalId: string,
	): ResultAsync<AuditLogsForPrincipalResponse, SdkError> {
		return this.client.request<AuditLogsForPrincipalResponse>(
			(httpClient, headers) =>
				sdk.auditLogsByPrincipal({
					client: httpClient,
					headers,
					path: { principalId },
				}),
		);
	}
}
