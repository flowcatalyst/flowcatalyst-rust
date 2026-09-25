// Stays hand-rolled: BFF endpoint — deliberately stripped from the OpenAPI
// spec (StripBFFPaths), so no generated types exist for it.
import { bffFetch } from "./client";

/**
 * Dashboard summary stats. Mixes exact counts (control plane) with
 * approximate counts derived from `pg_class.reltuples` (message plane —
 * rendered with `~` prefix in the UI to make the approximation explicit).
 */
export interface DashboardStats {
	totalClients: number;
	activeUsers: number;
	rolesDefined: number;
	eventsApprox: number;
	dispatchJobsApprox: number;
	auditLogsApprox: number;
	loginAttemptsApprox: number;
}

export const dashboardApi = {
	stats(): Promise<DashboardStats> {
		return bffFetch("/dashboard/stats");
	},
};
