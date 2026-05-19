import { apiFetch } from "./client";

export type ApplicationType = "APPLICATION" | "INTEGRATION";

export interface Application {
	id: string;
	type: ApplicationType;
	code: string;
	name: string;
	description?: string;
	defaultBaseUrl?: string;
	iconUrl?: string;
	website?: string;
	logo?: string;
	logoMimeType?: string;
	serviceAccountId?: string;
	/**
	 * True iff this application has a login OAuth client provisioned.
	 * Set by the detail endpoint; absent on list responses.
	 */
	hasLoginClient?: boolean;
	active: boolean;
	createdAt: string;
	updatedAt: string;
}

export interface ServiceAccountCredentials {
	principalId: string;
	name: string;
	oauthClient: {
		id: string;
		clientId: string;
		clientSecret: string; // Only available at creation time
	};
}

export interface ApplicationWithServiceAccount extends Application {
	serviceAccount?: ServiceAccountCredentials;
}

export interface ApplicationListResponse {
	applications: Application[];
	total: number;
}

export interface CreateApplicationRequest {
	code: string;
	name: string;
	description?: string;
	defaultBaseUrl?: string;
	iconUrl?: string;
	website?: string;
	logo?: string;
	logoMimeType?: string;
	type?: ApplicationType; // Defaults to APPLICATION
}

export interface UpdateApplicationRequest {
	name?: string;
	description?: string;
	defaultBaseUrl?: string;
	iconUrl?: string;
	website?: string;
	logo?: string;
	logoMimeType?: string;
}

export interface ListApplicationsOptions {
	activeOnly?: boolean;
	type?: ApplicationType;
}

export const applicationsApi = {
	list(
		options: ListApplicationsOptions = {},
	): Promise<ApplicationListResponse> {
		const params = new URLSearchParams();
		if (options.activeOnly) params.append("activeOnly", "true");
		if (options.type) params.append("type", options.type);
		const queryString = params.toString();
		return apiFetch(
			`/applications${queryString ? `?${queryString}` : ""}`,
		);
	},

	/**
	 * List only user-facing applications (type = APPLICATION).
	 * Use this when populating selectors for assigning apps to clients/users.
	 */
	listApplicationsOnly(activeOnly = true): Promise<ApplicationListResponse> {
		return this.list({ activeOnly, type: "APPLICATION" });
	},

	/**
	 * List only integrations (type = INTEGRATION).
	 */
	listIntegrationsOnly(activeOnly = true): Promise<ApplicationListResponse> {
		return this.list({ activeOnly, type: "INTEGRATION" });
	},

	get(id: string): Promise<Application> {
		return apiFetch(`/applications/${id}`);
	},

	getByCode(code: string): Promise<Application> {
		return apiFetch(`/applications/by-code/${code}`);
	},

	/**
	 * Create a new application or integration.
	 * Returns the application with service account credentials (only available at creation time).
	 */
	create(
		data: CreateApplicationRequest,
	): Promise<ApplicationWithServiceAccount> {
		return apiFetch("/applications", {
			method: "POST",
			body: JSON.stringify(data),
		});
	},

	update(id: string, data: UpdateApplicationRequest): Promise<Application> {
		return apiFetch(`/applications/${id}`, {
			method: "PUT",
			body: JSON.stringify(data),
		});
	},

	activate(id: string): Promise<Application> {
		return apiFetch(`/applications/${id}/activate`, { method: "POST" });
	},

	deactivate(id: string): Promise<Application> {
		return apiFetch(`/applications/${id}/deactivate`, { method: "POST" });
	},

	delete(id: string): Promise<void> {
		return apiFetch(`/applications/${id}`, { method: "DELETE" });
	},

	/**
	 * Provision a service account for an existing application.
	 * Returns the credentials (only available at provisioning time).
	 */
	provisionServiceAccount(id: string): Promise<{
		message: string;
		serviceAccount: ServiceAccountCredentials;
	}> {
		return apiFetch(`/applications/${id}/provision-service-account`, {
			method: "POST",
		});
	},

	/**
	 * Provision a user-login OAuth client for an existing application.
	 *
	 * - `PUBLIC` clients (default — SPAs, native apps) enforce PKCE and return
	 *   no `clientSecret`; protect the `clientId` and rely on the PKCE flow.
	 * - `CONFIDENTIAL` clients (server-rendered apps) get a `clientSecret`
	 *   returned exactly once.
	 *
	 * 409 if a login client already exists for the application — rotate the
	 * existing one via the OAuth Clients page, or delete it before
	 * re-provisioning.
	 */
	provisionLoginClient(
		id: string,
		body: ProvisionLoginClientRequest,
	): Promise<{
		message: string;
		loginClient: LoginClientCredentials;
	}> {
		return apiFetch(`/applications/${id}/provision-login-client`, {
			method: "POST",
			body: JSON.stringify(body),
		});
	},
};

/** Request body for `provisionLoginClient`. */
export interface ProvisionLoginClientRequest {
	clientType?: "PUBLIC" | "CONFIDENTIAL";
	redirectUris: string[];
	allowedOrigins?: string[];
}

/** Credentials returned from `provisionLoginClient`. */
export interface LoginClientCredentials {
	clientType: "PUBLIC" | "CONFIDENTIAL";
	redirectUris: string[];
	oauthClient: {
		id: string;
		clientId: string;
		/** Only present for CONFIDENTIAL clients. */
		clientSecret?: string;
	};
}
