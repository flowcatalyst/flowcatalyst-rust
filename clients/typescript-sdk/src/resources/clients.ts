/**
 * Clients Resource
 *
 * Manage clients (tenants) in the platform.
 */

import type { ResultAsync } from "neverthrow";
import type { SdkError } from "../errors";
import type { FlowCatalystClient } from "../client";
import * as sdk from "../generated/sdk.gen";
import type {
	GetApiClientsResponse,
	GetApiClientsByIdResponse,
	GetApiClientsByIdApplicationsResponse,
	GetApiClientsSearchResponse,
	PutApiClientsByIdApplicationsData,
	PostApiClientsData,
	PostApiClientsByIdNotesData,
	PostApiClientsByIdNotesResponse,
	PutApiClientsByIdData,
} from "../generated/types.gen";

export type ClientListResponse = GetApiClientsResponse;
export type ClientDto = GetApiClientsByIdResponse;
export type ClientApplicationsResponse =
	GetApiClientsByIdApplicationsResponse;
export type ClientSearchResponse = GetApiClientsSearchResponse;
export type AddNoteRequest = PostApiClientsByIdNotesData["body"];
export type AddNoteResponse = PostApiClientsByIdNotesResponse;
export type CreateClientRequest = PostApiClientsData["body"];
export type UpdateClientRequest = PutApiClientsByIdData["body"];
export type UpdateClientApplicationsRequest =
	PutApiClientsByIdApplicationsData["body"];

/**
 * Response for status change operations (enable/disable).
 */
export interface StatusResponse {
	message: string;
}

/**
 * Clients resource for managing platform clients (tenants).
 */
export class ClientsResource {
	private readonly client: FlowCatalystClient;

	constructor(client: FlowCatalystClient) {
		this.client = client;
	}

	/**
	 * List all clients.
	 */
	list(): ResultAsync<ClientListResponse, SdkError> {
		return this.client.request<ClientListResponse>((httpClient, headers) =>
			sdk.getApiClients({
				client: httpClient,
				headers,
			}),
		);
	}

	/**
	 * Get a client by ID.
	 */
	get(id: string): ResultAsync<ClientDto, SdkError> {
		return this.client.request<ClientDto>((httpClient, headers) =>
			sdk.getApiClientsById({
				client: httpClient,
				headers,
				path: { id },
			}),
		);
	}

	/**
	 * Get a client by identifier.
	 */
	getByIdentifier(identifier: string): ResultAsync<ClientDto, SdkError> {
		return this.client.request<ClientDto>((httpClient, headers) =>
			sdk.getApiClientsByIdentifierByIdentifier({
				client: httpClient,
				headers,
				path: { identifier },
			}),
		);
	}

	/**
	 * Create a new client.
	 */
	create(data: CreateClientRequest): ResultAsync<ClientDto, SdkError> {
		return this.client.request<ClientDto>((httpClient, headers) =>
			sdk.postApiClients({
				client: httpClient,
				headers,
				body: data,
			}),
		);
	}

	/**
	 * Update a client.
	 */
	update(
		id: string,
		data: UpdateClientRequest,
	): ResultAsync<ClientDto, SdkError> {
		return this.client.request<ClientDto>((httpClient, headers) =>
			sdk.putApiClientsById({
				client: httpClient,
				headers,
				path: { id },
				body: data,
			}),
		);
	}

	/**
	 * Activate a client.
	 */
	activate(id: string): ResultAsync<ClientDto, SdkError> {
		return this.client.request<ClientDto>((httpClient, headers) =>
			sdk.postApiClientsByIdActivate({
				client: httpClient,
				headers,
				path: { id },
			}),
		);
	}

	/**
	 * Deactivate a client.
	 */
	deactivate(id: string, reason: string): ResultAsync<ClientDto, SdkError> {
		return this.client.request<ClientDto>((httpClient, headers) =>
			sdk.postApiClientsByIdDeactivate({
				client: httpClient,
				headers,
				path: { id },
				body: { reason },
			}),
		);
	}

	/**
	 * Suspend a client with a reason.
	 */
	suspend(id: string, reason: string): ResultAsync<ClientDto, SdkError> {
		return this.client.request<ClientDto>((httpClient, headers) =>
			sdk.postApiClientsByIdSuspend({
				client: httpClient,
				headers,
				path: { id },
				body: { reason },
			}),
		);
	}

	/**
	 * Get applications configured for a client.
	 */
	getApplications(
		id: string,
	): ResultAsync<ClientApplicationsResponse, SdkError> {
		return this.client.request<ClientApplicationsResponse>(
			(httpClient, headers) =>
				sdk.getApiClientsByIdApplications({
					client: httpClient,
					headers,
					path: { id },
				}),
		);
	}

	/**
	 * Update the applications configured for a client.
	 */
	updateApplications(
		id: string,
		data: UpdateClientApplicationsRequest,
	): ResultAsync<ClientApplicationsResponse, SdkError> {
		return this.client.request<ClientApplicationsResponse>(
			(httpClient, headers) =>
				sdk.putApiClientsByIdApplications({
					client: httpClient,
					headers,
					path: { id },
					body: data,
				}),
		);
	}

	/**
	 * Enable an application for a client.
	 */
	enableApplication(
		clientId: string,
		applicationId: string,
	): ResultAsync<StatusResponse, SdkError> {
		return this.client.request<StatusResponse>((httpClient, headers) =>
			sdk.postApiClientsByIdApplicationsByAppIdEnable({
				client: httpClient,
				headers,
				path: { id: clientId, application_id: applicationId },
			}),
		);
	}

	/**
	 * Disable an application for a client.
	 */
	disableApplication(
		clientId: string,
		applicationId: string,
	): ResultAsync<StatusResponse, SdkError> {
		return this.client.request<StatusResponse>((httpClient, headers) =>
			sdk.postApiClientsByIdApplicationsByAppIdDisable({
				client: httpClient,
				headers,
				path: { id: clientId, application_id: applicationId },
			}),
		);
	}

	/**
	 * Search clients by name or identifier.
	 */
	search(query: string): ResultAsync<ClientSearchResponse, SdkError> {
		return this.client.request<ClientSearchResponse>((httpClient, headers) =>
			sdk.getApiClientsSearch({
				client: httpClient,
				headers,
				query: { q: query },
			}),
		);
	}

	/**
	 * Add a note to a client's audit history.
	 */
	addNote(
		id: string,
		category: string,
		text: string,
	): ResultAsync<AddNoteResponse, SdkError> {
		return this.client.request<AddNoteResponse>((httpClient, headers) =>
			sdk.postApiClientsByIdNotes({
				client: httpClient,
				headers,
				path: { id },
				body: { category, text },
			}),
		);
	}
}
