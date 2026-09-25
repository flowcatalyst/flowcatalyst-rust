// Stays hand-rolled: BFF endpoint — deliberately stripped from the OpenAPI
// spec (StripBFFPaths), so no generated types exist for it.
import { bffFetch } from "./client";

export type ProcessStatus = "CURRENT" | "ARCHIVED";
export type ProcessSource = "CODE" | "API" | "UI";

export interface Process {
	id: string;
	code: string;
	name: string;
	description: string | null;
	status: ProcessStatus;
	source: ProcessSource;
	application: string;
	subdomain: string;
	processName: string;
	body: string;
	diagramType: string;
	tags: string[];
	createdAt: string;
	updatedAt: string;
}

export interface ProcessListResponse {
	items: Process[];
}

export interface ProcessFilters {
	application?: string;
	subdomain?: string;
	status?: ProcessStatus;
	search?: string;
}

export interface CreateProcessRequest {
	code: string;
	name: string;
	description?: string;
	body?: string;
	diagramType?: string;
	tags?: string[];
}

export interface UpdateProcessRequest {
	name?: string;
	description?: string;
	body?: string;
	diagramType?: string;
	tags?: string[];
}

// All endpoints share the same router; the BFF mount accepts the SPA's
// cookie-based session, so we hit /bff/processes for browser traffic.
export const processesApi = {
	list(filters: ProcessFilters = {}): Promise<ProcessListResponse> {
		const params = new URLSearchParams();
		if (filters.application) params.set("application", filters.application);
		if (filters.subdomain) params.set("subdomain", filters.subdomain);
		if (filters.status) params.set("status", filters.status);
		if (filters.search) params.set("search", filters.search);
		const q = params.toString();
		return bffFetch(`/processes${q ? `?${q}` : ""}`);
	},

	get(id: string): Promise<Process> {
		return bffFetch(`/processes/${id}`);
	},

	getByCode(code: string): Promise<Process> {
		return bffFetch(`/processes/by-code/${encodeURIComponent(code)}`);
	},

	create(data: CreateProcessRequest): Promise<{ id: string }> {
		return bffFetch("/processes", {
			method: "POST",
			body: JSON.stringify(data),
		});
	},

	update(id: string, data: UpdateProcessRequest): Promise<void> {
		return bffFetch(`/processes/${id}`, {
			method: "PUT",
			body: JSON.stringify(data),
		});
	},

	archive(id: string): Promise<void> {
		return bffFetch(`/processes/${id}/archive`, { method: "POST" });
	},

	delete(id: string): Promise<void> {
		return bffFetch(`/processes/${id}`, { method: "DELETE" });
	},
};
