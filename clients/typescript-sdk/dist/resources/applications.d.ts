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
}
//# sourceMappingURL=applications.d.ts.map