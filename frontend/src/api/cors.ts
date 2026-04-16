import { apiFetch } from "./client";

export interface CorsOrigin {
	id: string;
	origin: string;
	description: string | null;
	createdBy: string;
	createdAt: string;
}

export interface CorsOriginListResponse {
	corsOrigins: CorsOrigin[];
	total: number;
}

export interface CreateCorsOriginRequest {
	origin: string;
	description?: string;
}

export const corsApi = {
	list(): Promise<CorsOriginListResponse> {
		return apiFetch("/platform/cors");
	},

	get(id: string): Promise<CorsOrigin> {
		return apiFetch(`/platform/cors/${id}`);
	},

	getAllowed(): Promise<{ origins: string[] }> {
		return apiFetch("/platform/cors/allowed");
	},

	create(data: CreateCorsOriginRequest): Promise<CorsOrigin> {
		return apiFetch("/platform/cors", {
			method: "POST",
			body: JSON.stringify(data),
		});
	},

	delete(id: string): Promise<void> {
		return apiFetch(`/platform/cors/${id}`, {
			method: "DELETE",
		});
	},
};
