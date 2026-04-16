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
}
