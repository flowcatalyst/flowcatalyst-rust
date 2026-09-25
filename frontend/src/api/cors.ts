import { apiFetch } from "./client";
import type {
	AllowedOriginResponse,
	CorsOriginListResponse as GenCorsOriginListResponse,
	CreatedResponse,
	PublicAllowedResponse,
} from "./generated";

// Response types alias the generated contract (api/openapi.lock.json) so
// `vue-tsc` fails on backend drift. Aliased under the historical names so
// pages keep their imports. (`description`/`createdBy` are optional on the
// wire — not `| null` / required as the old hand-rolled type claimed — and
// the wire also carries `updatedAt`.)
export type CorsOrigin = AllowedOriginResponse;
export type CorsOriginListResponse = GenCorsOriginListResponse;

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

	getAllowed(): Promise<PublicAllowedResponse> {
		return apiFetch("/platform/cors/allowed");
	},

	// POST returns the standard created envelope `{ id }` (201), not the full
	// origin as the old hand-rolled type claimed. Re-fetch the list to show
	// the new row.
	create(data: CreateCorsOriginRequest): Promise<CreatedResponse> {
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
