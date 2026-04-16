/**
 * API client for Login Attempt operations.
 */

import { apiFetch } from "./client";

export interface LoginAttempt {
	id: string;
	attemptType: string;
	outcome: string;
	failureReason: string | null;
	identifier: string;
	principalId: string | null;
	ipAddress: string | null;
	userAgent: string | null;
	attemptedAt: string;
}

export interface LoginAttemptListResponse {
	items: LoginAttempt[];
	total: number;
	page: number;
	pageSize: number;
}

export interface LoginAttemptFilters {
	attemptType?: string;
	outcome?: string;
	identifier?: string;
	principalId?: string;
	dateFrom?: string;
	dateTo?: string;
	page?: number;
	pageSize?: number;
	sortField?: string;
	sortOrder?: string;
}

/**
 * Fetch login attempts with optional filters and pagination.
 */
export async function fetchLoginAttempts(
	filters: LoginAttemptFilters = {},
): Promise<LoginAttemptListResponse> {
	const params = new URLSearchParams();
	if (filters.attemptType) params.set("attemptType", filters.attemptType);
	if (filters.outcome) params.set("outcome", filters.outcome);
	if (filters.identifier) params.set("identifier", filters.identifier);
	if (filters.principalId) params.set("principalId", filters.principalId);
	if (filters.dateFrom) params.set("dateFrom", filters.dateFrom);
	if (filters.dateTo) params.set("dateTo", filters.dateTo);
	if (filters.page !== undefined) params.set("page", String(filters.page));
	if (filters.pageSize !== undefined)
		params.set("pageSize", String(filters.pageSize));
	if (filters.sortField) params.set("sortField", filters.sortField);
	if (filters.sortOrder) params.set("sortOrder", filters.sortOrder);

	const query = params.toString();
	return apiFetch<LoginAttemptListResponse>(
		`/login-attempts${query ? `?${query}` : ""}`,
	);
}
