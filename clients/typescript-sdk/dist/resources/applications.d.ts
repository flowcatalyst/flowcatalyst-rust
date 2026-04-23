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
export declare class ApplicationsResource {
    private readonly client;
    constructor(client: FlowCatalystClient);
    /**
     * List all applications.
     */
    list(): ResultAsync<ApplicationListResponse, SdkError>;
    /**
     * Get an application by ID.
     */
    get(id: string): ResultAsync<ApplicationResponse, SdkError>;
    /**
     * Get an application by code.
     */
    getByCode(code: string): ResultAsync<ApplicationResponse, SdkError>;
    /**
     * Create a new application.
     */
    create(data: CreateApplicationRequest): ResultAsync<ApplicationResponse, SdkError>;
    /**
     * Update an application.
     */
    update(id: string, data: UpdateApplicationRequest): ResultAsync<ApplicationResponse, SdkError>;
    /**
     * Delete an application.
     */
    delete(id: string): ResultAsync<unknown, SdkError>;
    /**
     * Activate an application.
     */
    activate(id: string): ResultAsync<ApplicationResponse, SdkError>;
    /**
     * Deactivate an application.
     */
    deactivate(id: string): ResultAsync<ApplicationResponse, SdkError>;
    /**
     * Provision a service account for an application.
     */
    provisionServiceAccount(id: string): ResultAsync<CreateServiceAccountResponse, SdkError>;
    /**
     * Get the service account attached to an application.
     */
    getServiceAccount(id: string): ResultAsync<ServiceAccountResponse, SdkError>;
    /**
     * List roles defined for an application.
     */
    listRoles(id: string): ResultAsync<ApplicationRoleResponse[], SdkError>;
    /**
     * List per-client configs for an application.
     */
    listClients(id: string): ResultAsync<ClientConfigsResponse, SdkError>;
    /**
     * Update the per-client config for an application.
     */
    updateClientConfig(id: string, clientId: string, data: ClientConfigRequest): ResultAsync<ClientConfigResponse, SdkError>;
    /**
     * Enable an application for a specific client.
     */
    enableForClient(id: string, clientId: string): ResultAsync<ClientConfigResponse, SdkError>;
    /**
     * Disable an application for a specific client.
     */
    disableForClient(id: string, clientId: string): ResultAsync<ClientConfigResponse, SdkError>;
}
//# sourceMappingURL=applications.d.ts.map