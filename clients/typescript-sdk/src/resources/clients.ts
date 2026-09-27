/**
 * Clients Resource
 *
 * Manage clients (tenants) in the platform.
 */

import type { ResultAsync } from "neverthrow";
import type { SdkError } from "../errors.js";
import type { FlowCatalystClient } from "../client.js";
import * as sdk from "../generated/sdk.gen.js";
import type {
	ListClientsResponse,
	GetClientResponse,
	GetClientApplicationsResponse,
	SearchClientsByQueryResponse,
	UpdateClientApplicationsData,
	CreateClientData,
	CreatedResponse,
	StatusChangeResponse,
	AddClientNoteData,
	AddClientNoteResponse,
	UpdateClientData,
} from "../generated/types.gen.js";

export type ClientListResponse = ListClientsResponse;
export type ClientDto = GetClientResponse;
export type ClientApplicationsResponse = GetClientApplicationsResponse;
export type ClientSearchResponse = SearchClientsByQueryResponse;
export type AddNoteRequest = AddClientNoteData["body"];
export type AddNoteResponse = AddClientNoteResponse;
export type CreateClientRequest = CreateClientData["body"];
export type UpdateClientRequest = UpdateClientData["body"];
export type UpdateClientApplicationsRequest =
	UpdateClientApplicationsData["body"];

/**
 * @deprecated `enableApplication` / `disableApplication` resolve `void`: the
 * platform answers them `204 No Content`.
 */
export interface StatusResponse {
	message: string;
}

/** The body of a create: the new entity's id only. */
export type CreateClientResponse = CreatedResponse;

/** `{ message }`, the platform's answer to activate / deactivate / suspend. */
export type ClientStatusChangeResponse = StatusChangeResponse;

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
			sdk.listClients({
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
			sdk.getClient({
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
			sdk.getClientByIdentifier({
				client: httpClient,
				headers,
				path: { identifier },
			}),
		);
	}

	/**
	 * Create a new client. The platform answers `201 { id }`; call
	 * `get(id)` for the full entity.
	 */
	create(
		data: CreateClientRequest,
	): ResultAsync<CreateClientResponse, SdkError> {
		return this.client.request<CreateClientResponse>((httpClient, headers) =>
			sdk.createClient({
				client: httpClient,
				headers,
				body: data,
			}),
		);
	}

	/**
	 * Update a client. The platform answers `204 No Content`; call
	 * `get(id)` to read the result.
	 */
	update(id: string, data: UpdateClientRequest): ResultAsync<void, SdkError> {
		return this.client
			.request<unknown>((httpClient, headers) =>
				sdk.updateClient({
					client: httpClient,
					headers,
					path: { id },
					body: data,
				}),
			)
			.map((): void => undefined);
	}

	/**
	 * Activate a client. The platform answers `{ message }`; call `get(id)`
	 * for the client.
	 */
	activate(id: string): ResultAsync<ClientStatusChangeResponse, SdkError> {
		return this.client.request<ClientStatusChangeResponse>((httpClient, headers) =>
			sdk.activateClient({
				client: httpClient,
				headers,
				path: { id },
			}),
		);
	}

	/**
	 * Deactivate a client. The platform answers `{ message }`; call
	 * `get(id)` for the client.
	 */
	deactivate(
		id: string,
		reason: string,
	): ResultAsync<ClientStatusChangeResponse, SdkError> {
		return this.client.request<ClientStatusChangeResponse>((httpClient, headers) =>
			sdk.deactivateClient({
				client: httpClient,
				headers,
				path: { id },
				body: { reason },
			}),
		);
	}

	/**
	 * Suspend a client with a reason. The platform answers `{ message }`;
	 * call `get(id)` for the client.
	 */
	suspend(
		id: string,
		reason: string,
	): ResultAsync<ClientStatusChangeResponse, SdkError> {
		return this.client.request<ClientStatusChangeResponse>((httpClient, headers) =>
			sdk.suspendClient({
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
				sdk.getClientApplications({
					client: httpClient,
					headers,
					path: { id },
				}),
		);
	}

	/**
	 * Update the applications configured for a client. The platform answers
	 * `204 No Content`; call `getApplications(id)` to read the result.
	 */
	updateApplications(
		id: string,
		data: UpdateClientApplicationsRequest,
	): ResultAsync<void, SdkError> {
		return this.client
			.request<unknown>((httpClient, headers) =>
				sdk.updateClientApplications({
					client: httpClient,
					headers,
					path: { id },
					body: data,
				}),
			)
			.map((): void => undefined);
	}

	/**
	 * Enable an application for a client. The platform answers `204 No Content`.
	 */
	enableApplication(
		clientId: string,
		applicationId: string,
	): ResultAsync<void, SdkError> {
		return this.client
			.request<unknown>((httpClient, headers) =>
				sdk.enableClientApplication({
					client: httpClient,
					headers,
					path: { id: clientId, applicationId },
				}),
			)
			.map((): void => undefined);
	}

	/**
	 * Disable an application for a client. The platform answers `204 No Content`.
	 */
	disableApplication(
		clientId: string,
		applicationId: string,
	): ResultAsync<void, SdkError> {
		return this.client
			.request<unknown>((httpClient, headers) =>
				sdk.disableClientApplication({
					client: httpClient,
					headers,
					path: { id: clientId, applicationId },
				}),
			)
			.map((): void => undefined);
	}

	/**
	 * Search clients by name or identifier.
	 */
	search(query: string): ResultAsync<ClientSearchResponse, SdkError> {
		return this.client.request<ClientSearchResponse>((httpClient, headers) =>
			sdk.searchClientsByQuery({
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
			sdk.addClientNote({
				client: httpClient,
				headers,
				path: { id },
				body: { category, text },
			}),
		);
	}
}
