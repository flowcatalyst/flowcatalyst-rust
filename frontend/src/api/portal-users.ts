import { apiFetch } from "./client";
import type {
	PortalUserAppRef,
	PortalUserListItem,
	PortalUserListResponse as GenPortalUserListResponse,
	StatusChangeResponse,
} from "./generated";

// Portal identity plane admin surface (/api/portal-users). Response types
// alias the generated contract so vue-tsc fails on backend drift. Invites
// are deliberately absent: the portal app initiates them (POST
// /api/portal-users with its portalAppCode), never the platform UI.
export type PortalUser = PortalUserListItem;
export type PortalUserApp = PortalUserAppRef;
export type PortalUserListResponse = GenPortalUserListResponse;
export type PortalUserState =
	| "INVITED"
	| "INVITE_EXPIRED"
	| "ACTIVE"
	| "SUSPENDED";

export interface PortalUserSearch {
	clientId: string;
	// Prefix (TERM%) on email and name.
	q?: string;
	portalAppCode?: string;
	// Only users granted no portal app (exclusive with portalAppCode).
	unassigned?: boolean;
	page?: number;
	size?: number;
}

export const portalUsersApi = {
	list(params: PortalUserSearch): Promise<PortalUserListResponse> {
		const qs = new URLSearchParams({ clientId: params.clientId });
		if (params.q) qs.set("q", params.q);
		if (params.portalAppCode) qs.set("portalAppCode", params.portalAppCode);
		if (params.unassigned) qs.set("unassigned", "true");
		if (params.page !== undefined) qs.set("page", String(params.page));
		if (params.size !== undefined) qs.set("size", String(params.size));
		return apiFetch(`/portal-users?${qs.toString()}`);
	},

	activate(id: string, clientId: string): Promise<StatusChangeResponse> {
		return apiFetch(`/portal-users/${id}/activate`, {
			method: "POST",
			body: JSON.stringify({ clientId }),
		});
	},

	deactivate(id: string, clientId: string): Promise<StatusChangeResponse> {
		return apiFetch(`/portal-users/${id}/deactivate`, {
			method: "POST",
			body: JSON.stringify({ clientId }),
		});
	},

	remove(id: string, clientId: string): Promise<StatusChangeResponse> {
		return apiFetch(
			`/portal-users/${id}?clientId=${encodeURIComponent(clientId)}`,
			{ method: "DELETE" },
		);
	},

	grantApp(
		id: string,
		clientId: string,
		portalAppCode: string,
	): Promise<StatusChangeResponse> {
		return apiFetch(`/portal-users/${id}/apps`, {
			method: "POST",
			body: JSON.stringify({ clientId, portalAppCode }),
		});
	},

	revokeApp(
		id: string,
		clientId: string,
		portalAppCode: string,
	): Promise<StatusChangeResponse> {
		return apiFetch(
			`/portal-users/${id}/apps/${encodeURIComponent(portalAppCode)}?clientId=${encodeURIComponent(clientId)}`,
			{ method: "DELETE" },
		);
	},
};
