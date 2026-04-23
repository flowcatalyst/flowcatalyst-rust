/**
 * Applications Resource
 *
 * Manage applications in the platform.
 *
 * Uses direct HTTP calls since generated SDK functions are not yet available
 * (OpenAPI spec does not include /api/applications routes). Will be
 * migrated to generated functions once the spec is updated.
 */

import type { ResultAsync } from "neverthrow";
import type { SdkError } from "../errors";
import type { FlowCatalystClient } from "../client";

export interface ApplicationResponse {
	id: string;
	code: string;
	name: string;
	description: string | null;
	type: string;
	active: boolean;
	createdAt: string;
	updatedAt: string;
}

export interface ApplicationListResponse {
	applications: ApplicationResponse[];
	total: number;
}

export interface CreateApplicationRequest {
	code: string;
	name: string;
	description?: string | null;
	type: string;
}

export interface UpdateApplicationRequest {
	name?: string;
	description?: string | null;
}

export interface CreateServiceAccountResponse {
	serviceAccountId: string;
	clientId: string;
	clientSecret: string;
}

export interface ServiceAccountResponse {
	id: string;
	code: string;
	name: string;
	description?: string | null;
	active: boolean;
	applicationId?: string | null;
	createdAt: string;
}

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
	clientName?: string | null;
	clientIdentifier?: string | null;
	enabled: boolean;
	baseUrlOverride?: string | null;
	effectiveBaseUrl?: string | null;
	config?: Record<string, unknown> | null;
}

export interface ClientConfigsResponse {
	clientConfigs: ClientConfigResponse[];
	total?: number;
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
	 * List all applications.
	 */
	list(): ResultAsync<ApplicationListResponse, SdkError> {
		return this.client.request<ApplicationListResponse>((httpClient, headers) =>
			httpClient.get({
				url: "/api/applications",
				headers,
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
	 * Create a new application.
	 */
	create(
		data: CreateApplicationRequest,
	): ResultAsync<ApplicationResponse, SdkError> {
		return this.client.request<ApplicationResponse>((httpClient, headers) =>
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
	 * Update an application.
	 */
	update(
		id: string,
		data: UpdateApplicationRequest,
	): ResultAsync<ApplicationResponse, SdkError> {
		return this.client.request<ApplicationResponse>((httpClient, headers) =>
			httpClient.put({
				url: "/api/applications/{id}",
				headers: {
					...headers,
					"Content-Type": "application/json",
				},
				path: { id },
				body: data,
			}),
		);
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
	 */
	provisionServiceAccount(
		id: string,
	): ResultAsync<CreateServiceAccountResponse, SdkError> {
		return this.client.request<CreateServiceAccountResponse>(
			(httpClient, headers) =>
				httpClient.post({
					url: "/api/applications/{id}/provision-service-account",
					headers,
					path: { id },
				}),
		);
	}

	/**
	 * Get the service account attached to an application.
	 */
	getServiceAccount(
		id: string,
	): ResultAsync<ServiceAccountResponse, SdkError> {
		return this.client.request<ServiceAccountResponse>((httpClient, headers) =>
			httpClient.get({
				url: "/api/applications/{id}/service-account",
				headers,
				path: { id },
			}),
		);
	}

	/**
	 * List roles defined for an application.
	 */
	listRoles(
		id: string,
	): ResultAsync<ApplicationRoleResponse[], SdkError> {
		return this.client.request<ApplicationRoleResponse[]>(
			(httpClient, headers) =>
				httpClient.get({
					url: "/api/applications/{id}/roles",
					headers,
					path: { id },
				}),
		);
	}

	/**
	 * List per-client configs for an application.
	 */
	listClients(
		id: string,
	): ResultAsync<ClientConfigsResponse, SdkError> {
		return this.client.request<ClientConfigsResponse>((httpClient, headers) =>
			httpClient.get({
				url: "/api/applications/{id}/clients",
				headers,
				path: { id },
			}),
		);
	}

	/**
	 * Update the per-client config for an application.
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
	 * Enable an application for a specific client.
	 */
	enableForClient(
		id: string,
		clientId: string,
	): ResultAsync<ClientConfigResponse, SdkError> {
		return this.client.request<ClientConfigResponse>((httpClient, headers) =>
			httpClient.post({
				url: "/api/applications/{id}/clients/{clientId}/enable",
				headers,
				path: { id, clientId },
			}),
		);
	}

	/**
	 * Disable an application for a specific client.
	 */
	disableForClient(
		id: string,
		clientId: string,
	): ResultAsync<ClientConfigResponse, SdkError> {
		return this.client.request<ClientConfigResponse>((httpClient, headers) =>
			httpClient.post({
				url: "/api/applications/{id}/clients/{clientId}/disable",
				headers,
				path: { id, clientId },
			}),
		);
	}
}
