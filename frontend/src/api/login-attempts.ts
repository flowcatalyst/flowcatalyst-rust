/**
 * API client for Login Attempt operations.
 */

import { apiFetch } from "./client";
import type {
	LoginAttemptListResponse as GenLoginAttemptListResponse,
	LoginAttemptResponse,
} from "./generated";

// Response types alias the generated contract (api/openapi.lock.json) so
// `vue-tsc` fails on backend drift. Aliased under the historical names so
// pages keep their imports.
export type LoginAttempt = LoginAttemptResponse;

/**
 * Cursor-paginated login attempts. Backend keysets on
 * `(attemptedAt, id) DESC`; `iam_login_attempts` is unbounded so we never
 * count.
 */
export type LoginAttemptListResponse = GenLoginAttemptListResponse;

export interface LoginAttemptFilters {
	attemptType?: string;
	outcome?: string;
	identifier?: string;
	principalId?: string;
	dateFrom?: string;
	dateTo?: string;
	/** Opaque cursor returned by a previous response. */
	after?: string | undefined;
	pageSize?: number;
}

/**
 * Fetch a page of login attempts (cursor-paginated).
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
	if (filters.after) params.set("after", filters.after);
	if (filters.pageSize !== undefined)
		params.set("pageSize", String(filters.pageSize));

	const query = params.toString();
	return apiFetch<LoginAttemptListResponse>(
		`/login-attempts${query ? `?${query}` : ""}`,
	);
}
