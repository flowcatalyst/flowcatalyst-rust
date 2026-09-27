/**
 * Applications Resource
 *
 * Manage applications in the platform.
 *
 * Uses direct HTTP calls against the platform's `/api/applications` routes.
 * Request and response shapes follow the platform's OpenAPI contract
 * (`ApplicationResponse`, `CreatedResponse`, `ClientConfigListResponse`, …).
 */

import { errAsync, type ResultAsync } from "neverthrow";
import type { SdkError } from "../errors.js";
import { notFoundError } from "../errors.js";
import type { FlowCatalystClient } from "../client.js";

export interface ApplicationResponse {
	id: string;
	code: string;
	name: string;
	/** Omitted by the platform when unset. */
	description?: string | null;
	type: string;
	active: boolean;
	defaultBaseUrl?: string;
	iconUrl?: string;
	website?: string;
	logo?: string;
	logoMimeType?: string;
	/** The application's service account, once one is provisioned or attached. */
	serviceAccountId?: string;
	/** Whether a login OAuth client exists for the application. */
	hasLoginClient?: boolean;
	createdAt: string;
	updatedAt: string;
}

export interface ApplicationListResponse {
	applications: ApplicationResponse[];
	total: number;
}

export interface ApplicationListFilters {
	/** Filter by application type. */
	type?: string;
	/** Filter by active flag (`"true"` / `"false"`). */
	active?: string;
}

export interface CreateApplicationRequest {
	code: string;
	name: string;
	description?: string | null;
	/** Optional; the platform applies its default when omitted. */
	type?: string;
	defaultBaseUrl?: string;
	iconUrl?: string;
	website?: string;
	logo?: string;
	logoMimeType?: string;
}

export interface UpdateApplicationRequest {
	name?: string;
	description?: string | null;
	defaultBaseUrl?: string;
	iconUrl?: string;
	website?: string;
	logo?: string;
	logoMimeType?: string;
}

/** The body of a create: the new entity's id only. */
export interface CreatedResponse {
	id: string;
}

/** OAuth client credentials. `clientSecret` is plaintext and returned once. */
export interface ApplicationOAuthClientCredentials {
	id: string;
	clientId: string;
	clientSecret?: string;
}

export interface ApplicationServiceAccountCredentials {
	principalId: string;
	name: string;
	oauthClient: ApplicationOAuthClientCredentials;
}

/**
 * Response of `provisionServiceAccount`. The platform returns the one-time
 * secret nested under `serviceAccount.oauthClient.clientSecret`; the flat
 * `clientId` / `clientSecret` members are copies of it, kept for callers of
 * earlier SDK versions.
 */
export interface CreateServiceAccountResponse {
	message?: string;
	serviceAccount?: ApplicationServiceAccountCredentials;
	/**
	 * @deprecated The platform does not return the service account's id
	 * here; read `serviceAccountId` from `get(applicationId)` instead.
	 * Set only when the platform sends it.
	 */
	serviceAccountId?: string;
	/** @deprecated Use `serviceAccount.oauthClient.clientId`. */
	clientId?: string;
	/** @deprecated Use `serviceAccount.oauthClient.clientSecret`. */
	clientSecret?: string;
}

export interface ServiceAccountResponse {
	id: string;
	code: string;
	name: string;
	description?: string | null;
	active: boolean;
	applicationId?: string | null;
	clientIds?: string[];
	authType?: string;
	roles?: string[];
	principalId?: string;
	oauthClientId?: string;
	scope?: string;
	lastUsedAt?: string;
	createdAt: string;
	updatedAt?: string;
}

/** Response of `listRoles`: the role names registered for the application. */
export interface ApplicationRolesResponse {
	roles: string[];
}

/**
 * @deprecated The platform's `GET /api/applications/by-id/{id}/roles`
 * returns role names only (`ApplicationRolesResponse`).
 */
export interface ApplicationRoleResponse {
	id: string;
	code: string;
	displayName: string;
	description?: string | null;
	applicationCode: string;
	permissions: string[];
	source: string;
	clientManaged: boolean;
}

export interface ClientConfigRequest {
	enabled?: boolean;
	baseUrlOverride?: string | null;
	config?: Record<string, unknown> | null;
}

export interface ClientConfigResponse {
	id: string;
	applicationId: string;
	clientId: string;
	enabled: boolean;
	baseUrlOverride?: string | null;
	/** The per-client configuration document. */
	configJson?: unknown;
	createdAt?: string;
	updatedAt?: string;
	/** @deprecated Use `configJson`; this is a copy of it. */
	config?: Record<string, unknown> | null;
	/** @deprecated The platform does not return it. */
	clientName?: string | null;
	/** @deprecated The platform does not return it. */
	clientIdentifier?: string | null;
	/** @deprecated The platform does not return it. */
	effectiveBaseUrl?: string | null;
}

export interface ClientConfigsResponse {
	items: ClientConfigResponse[];
	/** @deprecated Use `items`; this is the same array. */
	clientConfigs: ClientConfigResponse[];
	/** @deprecated Use `items.length`. */
	total: number;
}

/** Copy `configJson` into the deprecated `config` alias. */
function withConfigAlias(c: ClientConfigResponse): ClientConfigResponse {
	if (c.config === undefined && c.configJson !== undefined) {
		return { ...c, config: c.configJson as Record<string, unknown> | null };
	}
	return c;
}

/**
 * Applications resource for managing platform applications.
 */
export class ApplicationsResource {
	private readonly client: FlowCatalystClient;

	constructor(client: FlowCatalystClient) {
		this.client = client;
	}

	/**
	 * List applications, optionally filtered by `type` / `active`.
	 */
	list(
		filters?: ApplicationListFilters,
	): ResultAsync<ApplicationListResponse, SdkError> {
		return this.client.request<ApplicationListResponse>((httpClient, headers) =>
			httpClient.get({
				url: "/api/applications",
				headers,
				...(filters ? { query: { ...filters } } : {}),
			}),
		);
	}

	/**
	 * Get an application by ID.
	 */
	get(id: string): ResultAsync<ApplicationResponse, SdkError> {
		return this.client.request<ApplicationResponse>((httpClient, headers) =>
			httpClient.get({
				url: "/api/applications/{id}",
				headers,
				path: { id },
			}),
		);
	}

	/**
	 * Get an application by code.
	 */
	getByCode(code: string): ResultAsync<ApplicationResponse, SdkError> {
		return this.client.request<ApplicationResponse>((httpClient, headers) =>
			httpClient.get({
				url: "/api/applications/by-code/{code}",
				headers,
				path: { code },
			}),
		);
	}

	/**
	 * Create a new application. The platform answers `201 { id }`; call
	 * `get(id)` for the full entity.
	 */
	create(
		data: CreateApplicationRequest,
	): ResultAsync<CreatedResponse, SdkError> {
		return this.client.request<CreatedResponse>((httpClient, headers) =>
			httpClient.post({
				url: "/api/applications",
				headers: {
					...headers,
					"Content-Type": "application/json",
				},
				body: data,
			}),
		);
	}

	/**
	 * Update an application. The platform answers `204 No Content`; call
	 * `get(id)` to read the result.
	 */
	update(
		id: string,
		data: UpdateApplicationRequest,
	): ResultAsync<void, SdkError> {
		return this.client
			.request<unknown>((httpClient, headers) =>
				httpClient.put({
					url: "/api/applications/{id}",
					headers: {
						...headers,
						"Content-Type": "application/json",
					},
					path: { id },
					body: data,
				}),
			)
			.map((): void => undefined);
	}

	/**
	 * Delete an application.
	 */
	delete(id: string): ResultAsync<unknown, SdkError> {
		return this.client.request<unknown>((httpClient, headers) =>
			httpClient.delete({
				url: "/api/applications/{id}",
				headers,
				path: { id },
			}),
		);
	}

	/**
	 * Activate an application.
	 */
	activate(id: string): ResultAsync<ApplicationResponse, SdkError> {
		return this.client.request<ApplicationResponse>((httpClient, headers) =>
			httpClient.post({
				url: "/api/applications/{id}/activate",
				headers,
				path: { id },
			}),
		);
	}

	/**
	 * Deactivate an application.
	 */
	deactivate(id: string): ResultAsync<ApplicationResponse, SdkError> {
		return this.client.request<ApplicationResponse>((httpClient, headers) =>
			httpClient.post({
				url: "/api/applications/{id}/deactivate",
				headers,
				path: { id },
			}),
		);
	}

	/**
	 * Provision a service account for an application.
	 *
	 * The OAuth client secret is returned once, at
	 * `serviceAccount.oauthClient.clientSecret` (also copied to the
	 * deprecated flat `clientSecret`). Store it immediately.
	 */
	provisionServiceAccount(
		id: string,
	): ResultAsync<CreateServiceAccountResponse, SdkError> {
		return this.client
			.request<CreateServiceAccountResponse>((httpClient, headers) =>
				httpClient.post({
					url: "/api/applications/{id}/provision-service-account",
					headers,
					path: { id },
				}),
			)
			.map((r) => {
				const oauth = r.serviceAccount?.oauthClient;
				return {
					...r,
					clientId: r.clientId ?? oauth?.clientId,
					clientSecret: r.clientSecret ?? oauth?.clientSecret,
				};
			});
	}

	/**
	 * Get the service account attached to an application.
	 *
	 * Reads the application's `serviceAccountId` and then fetches
	 * `GET /api/service-accounts/{serviceAccountId}`. Fails with a
	 * `not_found` error when the application has no service account.
	 */
	getServiceAccount(
		id: string,
	): ResultAsync<ServiceAccountResponse, SdkError> {
		return this.get(id).andThen((app) => {
			const serviceAccountId = app.serviceAccountId;
			if (!serviceAccountId) {
				return errAsync<ServiceAccountResponse, SdkError>(
					notFoundError(
						`Application ${id} has no service account`,
						"ServiceAccount",
						id,
					),
				);
			}
			return this.client.request<ServiceAccountResponse>(
				(httpClient, headers) =>
					httpClient.get({
						url: "/api/service-accounts/{id}",
						headers,
						path: { id: serviceAccountId },
					}),
			);
		});
	}

	/**
	 * List the names of the roles registered for an application.
	 *
	 * The platform returns `{ roles: string[] }` (role names only). Use
	 * `client.roles().listForApplication(id)` for full role objects.
	 */
	listRoles(id: string): ResultAsync<ApplicationRolesResponse, SdkError> {
		return this.client.request<ApplicationRolesResponse>(
			(httpClient, headers) =>
				httpClient.get({
					url: "/api/applications/by-id/{id}/roles",
					headers,
					path: { id },
				}),
		);
	}

	/**
	 * List per-client configs for an application. The platform returns
	 * `{ items }`; `clientConfigs` / `total` are kept as deprecated aliases.
	 */
	listClients(id: string): ResultAsync<ClientConfigsResponse, SdkError> {
		return this.client
			.request<{ items?: ClientConfigResponse[] }>((httpClient, headers) =>
				httpClient.get({
					url: "/api/applications/{id}/clients",
					headers,
					path: { id },
				}),
			)
			.map((r) => {
				const items = (r.items ?? []).map(withConfigAlias);
				return { items, clientConfigs: items, total: items.length };
			});
	}

	/**
	 * Get the config of one client for an application.
	 */
	getClientConfig(
		id: string,
		clientId: string,
	): ResultAsync<ClientConfigResponse, SdkError> {
		return this.client
			.request<ClientConfigResponse>((httpClient, headers) =>
				httpClient.get({
					url: "/api/applications/{id}/clients/{clientId}",
					headers,
					path: { id, clientId },
				}),
			)
			.map(withConfigAlias);
	}

	/**
	 * Update the per-client config for an application.
	 *
	 * @deprecated Only the FlowCatalyst Rust platform serves
	 * `PUT /api/applications/{id}/clients/{clientId}`; the Go platform
	 * does not (it serves only `GET` on that path). Use `enableForClient` / `disableForClient`, and
	 * `getClientConfig` to read a config.
	 */
	updateClientConfig(
		id: string,
		clientId: string,
		data: ClientConfigRequest,
	): ResultAsync<ClientConfigResponse, SdkError> {
		return this.client.request<ClientConfigResponse>((httpClient, headers) =>
			httpClient.put({
				url: "/api/applications/{id}/clients/{clientId}",
				headers: {
					...headers,
					"Content-Type": "application/json",
				},
				path: { id, clientId },
				body: data,
			}),
		);
	}

	/**
	 * Enable an application for a specific client. The platform answers
	 * `204 No Content`; call `getClientConfig` to read the result.
	 */
	enableForClient(id: string, clientId: string): ResultAsync<void, SdkError> {
		return this.client
			.request<unknown>((httpClient, headers) =>
				httpClient.post({
					url: "/api/applications/{id}/clients/{clientId}/enable",
					headers,
					path: { id, clientId },
				}),
			)
			.map((): void => undefined);
	}

	/**
	 * Disable an application for a specific client. The platform answers
	 * `204 No Content`; call `getClientConfig` to read the result.
	 */
	disableForClient(id: string, clientId: string): ResultAsync<void, SdkError> {
		return this.client
			.request<unknown>((httpClient, headers) =>
				httpClient.post({
					url: "/api/applications/{id}/clients/{clientId}/disable",
					headers,
					path: { id, clientId },
				}),
			)
			.map((): void => undefined);
	}
}
