/**
 * Processes Resource
 *
 * Manage process documentation — free-form workflow diagrams (typically
 * Mermaid source) describing how events, reactive aggregates, and dispatch
 * jobs compose into business processes inside an application.
 *
 * Codes follow the pattern `{application}:{subdomain}:{process-name}`, mirroring
 * EventType. The `body` field is stored verbatim and rendered client-side.
 */

import type { ResultAsync } from "neverthrow";
import type { SdkError } from "../errors.js";
import type { FlowCatalystClient } from "../client.js";
import * as sdk from "../generated/sdk.gen.js";
import type {
	ListProcessesResponse,
	GetProcessResponse,
	CreateProcessData,
	CreatedResponse,
	UpdateProcessData,
	SyncProcessesData,
	SyncProcessesResponse as SyncProcessesResponseType,
	ListProcessesData,
} from "../generated/types.gen.js";

/**
 * Pagination params (page/size).
 *
 * @deprecated The platform's `GET /api/processes` does not paginate; it
 * returns every matching process and ignores `page` / `size`.
 */
export type PaginationParams = {
	page?: number;
	size?: number;
};

export type ProcessListResponse = ListProcessesResponse;
export type ProcessResponse = GetProcessResponse;
export type CreateProcessRequest = CreateProcessData["body"];
export type UpdateProcessRequest = UpdateProcessData["body"];
export type SyncProcessesResponse = SyncProcessesResponseType;
export type CreateProcessResponse = CreatedResponse;

export interface ProcessFilters {
	status?: string;
	application?: string;
	subdomain?: string;
	/**
	 * @deprecated The platform's `GET /api/processes` has no `search`
	 * filter and ignores it.
	 */
	search?: string;
}

/**
 * Processes resource for managing workflow / process documentation.
 */
export class ProcessesResource {
	private readonly client: FlowCatalystClient;

	constructor(client: FlowCatalystClient) {
		this.client = client;
	}

	/** List processes with optional filters. */
	list(
		filters?: ProcessFilters,
		pagination?: PaginationParams,
	): ResultAsync<ProcessListResponse, SdkError> {
		return this.client.request<ProcessListResponse>((httpClient, headers) =>
			sdk.listProcesses({
				client: httpClient,
				headers,
				query: {
					...pagination,
					...filters,
				} as ListProcessesData["query"],
			}),
		);
	}

	/** Get a process by ID. */
	get(id: string): ResultAsync<ProcessResponse, SdkError> {
		return this.client.request<ProcessResponse>((httpClient, headers) =>
			sdk.getProcess({
				client: httpClient,
				headers,
				path: { id },
			}),
		);
	}

	/** Get a process by code (`{app}:{subdomain}:{process}`). */
	getByCode(code: string): ResultAsync<ProcessResponse, SdkError> {
		return this.client.request<ProcessResponse>((httpClient, headers) =>
			sdk.getProcessByCode({
				client: httpClient,
				headers,
				path: { code },
			}),
		);
	}

	/**
	 * Create a new process. The platform answers `201 { id }`; call
	 * `get(id)` for the full entity.
	 */
	create(
		data: CreateProcessRequest,
	): ResultAsync<CreateProcessResponse, SdkError> {
		return this.client.request<CreateProcessResponse>((httpClient, headers) =>
			sdk.createProcess({
				client: httpClient,
				headers,
				body: data,
			}),
		);
	}

	/**
	 * Update a process. The platform answers `204 No Content`; call
	 * `get(id)` to read the result.
	 */
	update(
		id: string,
		data: UpdateProcessRequest,
	): ResultAsync<void, SdkError> {
		return this.client
			.request<unknown>((httpClient, headers) =>
				sdk.updateProcess({
					client: httpClient,
					headers,
					path: { id },
					body: data,
				}),
			)
			.map((): void => undefined);
	}

	/**
	 * Archive a process (soft-delete): `POST /api/processes/{id}/archive`,
	 * answered `204 No Content`.
	 */
	archive(id: string): ResultAsync<void, SdkError> {
		return this.client
			.request<unknown>((httpClient, headers) =>
				sdk.archiveProcess({
					client: httpClient,
					headers,
					path: { id },
				}),
			)
			.map((): void => undefined);
	}

	/** Hard-delete a process. Only allowed once archived. */
	delete(id: string): ResultAsync<unknown, SdkError> {
		return this.client.request<unknown>((httpClient, headers) =>
			sdk.deleteProcess({
				client: httpClient,
				headers,
				path: { id },
			}),
		);
	}

	/**
	 * Sync processes for an application. The platform reconciles the
	 * provided list against existing API/CODE-sourced processes; UI-sourced
	 * processes are never touched.
	 *
	 * Calls `POST /api/applications/{applicationCode}/processes/sync`.
	 */
	sync(
		applicationCode: string,
		processes: SyncProcessesData["body"]["processes"],
		removeUnlisted = false,
	): ResultAsync<SyncProcessesResponse, SdkError> {
		return this.client.request<SyncProcessesResponse>((httpClient, headers) =>
			sdk.syncProcesses({
				client: httpClient,
				headers,
				path: { appCode: applicationCode },
				body: { processes },
				query: { removeUnlisted },
			}),
		);
	}
}
