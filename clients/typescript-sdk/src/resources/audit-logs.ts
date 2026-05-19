/**
 * Audit Logs Resource
 *
 * Query the platform's `iam_audit_logs` table — every UoW commit emits a
 * row here in addition to its domain event. Read-only by design.
 */

import type { ResultAsync } from "neverthrow";
import type { SdkError } from "../errors";
import type { FlowCatalystClient } from "../client";
import * as sdk from "../generated/sdk.gen";
import type {
	GetApiAuditLogsData,
	GetApiAuditLogsResponse,
	GetApiAuditLogsByIdResponse,
	GetApiAuditLogsRecentResponse,
	GetApiAuditLogsEntityByEntityTypeByEntityIdResponse,
	GetApiAuditLogsPrincipalByPrincipalIdResponse,
} from "../generated/types.gen";

export type AuditLogFilters = GetApiAuditLogsData["query"];
export type AuditLogListResponse = GetApiAuditLogsResponse;
export type AuditLogDto = GetApiAuditLogsByIdResponse;
export type RecentAuditLogsResponse = GetApiAuditLogsRecentResponse;
export type AuditLogsForEntityResponse =
	GetApiAuditLogsEntityByEntityTypeByEntityIdResponse;
export type AuditLogsForPrincipalResponse =
	GetApiAuditLogsPrincipalByPrincipalIdResponse;

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
			sdk.getApiAuditLogs({
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
			sdk.getApiAuditLogsById({
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
				sdk.getApiAuditLogsRecent({
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
				sdk.getApiAuditLogsEntityByEntityTypeByEntityId({
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
				sdk.getApiAuditLogsPrincipalByPrincipalId({
					client: httpClient,
					headers,
					path: { principalId },
				}),
		);
	}
}
