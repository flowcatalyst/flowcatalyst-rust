/**
 * Dispatch Pools Resource
 *
 * Manage dispatch pools for rate limiting and concurrency control.
 *
 * Uses direct HTTP calls since generated SDK functions are not yet available
 * (OpenAPI spec does not include /api/dispatch-pools routes). Will be
 * migrated to generated functions once the spec is updated.
 */
import type { ResultAsync } from "neverthrow";
import type { SdkError } from "../errors";
import type { FlowCatalystClient } from "../client";
export interface DispatchPoolDto {
    id: string;
    code: string;
    name: string;
    description: string | null;
    status: string;
    maxConcurrency: number;
    rateLimit: number | null;
    rateLimitWindow: number | null;
    clientId: string | null;
    applicationCode: string | null;
    createdAt: string;
    updatedAt: string;
}
export interface DispatchPoolListResponse {
    pools: DispatchPoolDto[];
    total: number;
}
export interface CreateDispatchPoolRequest {
    code: string;
    name: string;
    description?: string | null;
    maxConcurrency: number;
    rateLimit?: number | null;
    rateLimitWindow?: number | null;
    applicationCode?: string | null;
}
export interface UpdateDispatchPoolRequest {
    name?: string;
    description?: string | null;
    maxConcurrency?: number;
    rateLimit?: number | null;
    rateLimitWindow?: number | null;
}
export interface SyncDispatchPoolsResponse {
    created: number;
    updated: number;
    removed: number;
}
export interface DispatchPoolFilters {
    clientId?: string;
    status?: string;
    [key: string]: unknown;
}
/**
 * Dispatch Pools resource for managing rate limiting and concurrency.
 */
export declare class DispatchPoolsResource {
    private readonly client;
    constructor(client: FlowCatalystClient);
    /**
     * List all dispatch pools with optional filters.
     */
    list(filters?: DispatchPoolFilters): ResultAsync<DispatchPoolListResponse, SdkError>;
    /**
     * Get a dispatch pool by ID.
     */
    get(id: string): ResultAsync<DispatchPoolDto, SdkError>;
    /**
     * Create a new dispatch pool.
     */
    create(data: CreateDispatchPoolRequest): ResultAsync<DispatchPoolDto, SdkError>;
    /**
     * Update a dispatch pool.
     */
    update(id: string, data: UpdateDispatchPoolRequest): ResultAsync<DispatchPoolDto, SdkError>;
    /**
     * Delete (archive) a dispatch pool.
     */
    delete(id: string): ResultAsync<unknown, SdkError>;
    /**
     * Suspend a dispatch pool.
     */
    suspend(id: string): ResultAsync<DispatchPoolDto, SdkError>;
    /**
     * Activate a dispatch pool.
     */
    activate(id: string): ResultAsync<DispatchPoolDto, SdkError>;
    /**
     * Sync dispatch pools for an application.
     */
    sync(applicationCode: string, pools: Array<{
        code: string;
        name: string;
        description?: string | null;
        maxConcurrency: number;
        rateLimit?: number | null;
        rateLimitWindow?: number | null;
    }>, removeUnlisted?: boolean): ResultAsync<SyncDispatchPoolsResponse, SdkError>;
}
//# sourceMappingURL=dispatch-pools.d.ts.map