import { apiFetch } from "./client";
import type {
	AssignUnassignedResponse,
	CreatePortalAppResponse as GenCreatePortalAppResponse,
	PortalAppListResponse as GenPortalAppListResponse,
	PortalAppResponse,
	StatusChangeResponse,
} from "./generated";

// Portal apps (/api/portal-apps): the named portals a client runs. OAuth
// clients link to one (portalAppId) and portal users are granted per app.
export type PortalApp = PortalAppResponse;
export type PortalAppListResponse = GenPortalAppListResponse;
// The new app plus its auto-provisioned portal OAuth client; clientSecret
// (CONFIDENTIAL only) is shown exactly once.
export type CreatePortalAppResponse = GenCreatePortalAppResponse;
export type PortalClientType = "CONFIDENTIAL" | "PUBLIC";

export interface CreatePortalAppRequest {
	clientId: string;
	code: string;
	name: string;
	description?: string;
	// Callback URL(s) registered on the provisioned OAuth client.
	redirectUris?: string[];
	clientType?: PortalClientType;
}

export interface UpdatePortalAppRequest {
	clientId: string;
	name?: string;
	description?: string;
	active?: boolean;
}

export const portalAppsApi = {
	// Omit clientId (anchors only) to list every client's apps.
	list(clientId?: string): Promise<PortalAppListResponse> {
		return apiFetch(
			clientId
				? `/portal-apps?clientId=${encodeURIComponent(clientId)}`
				: "/portal-apps",
		);
	},

	create(body: CreatePortalAppRequest): Promise<CreatePortalAppResponse> {
		return apiFetch("/portal-apps", {
			method: "POST",
			body: JSON.stringify(body),
		});
	},

	update(id: string, body: UpdatePortalAppRequest): Promise<PortalApp> {
		return apiFetch(`/portal-apps/${id}`, {
			method: "PUT",
			body: JSON.stringify(body),
		});
	},

	// Grant this app to every one of the client's portal users that has no
	// portal app (users predating portal apps would otherwise be locked out
	// once their portal OAuth client is linked to an app).
	assignUnassigned(id: string, clientId: string): Promise<AssignUnassignedResponse> {
		return apiFetch(`/portal-apps/${id}/assign-unassigned`, {
			method: "POST",
			body: JSON.stringify({ clientId }),
		});
	},

	remove(id: string, clientId: string): Promise<StatusChangeResponse> {
		return apiFetch(
			`/portal-apps/${id}?clientId=${encodeURIComponent(clientId)}`,
			{ method: "DELETE" },
		);
	},
};
