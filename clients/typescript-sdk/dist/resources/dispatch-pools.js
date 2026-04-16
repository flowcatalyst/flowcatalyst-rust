/**
 * Dispatch Pools Resource
 *
 * Manage dispatch pools for rate limiting and concurrency control.
 *
 * Uses direct HTTP calls since generated SDK functions are not yet available
 * (OpenAPI spec does not include /api/dispatch-pools routes). Will be
 * migrated to generated functions once the spec is updated.
 */
/**
 * Dispatch Pools resource for managing rate limiting and concurrency.
 */
export class DispatchPoolsResource {
    constructor(client) {
        this.client = client;
    }
    /**
     * List all dispatch pools with optional filters.
     */
    list(filters) {
        return this.client.request((httpClient, headers) => httpClient.get({
            url: "/api/dispatch-pools",
            headers,
            query: filters,
        }));
    }
    /**
     * Get a dispatch pool by ID.
     */
    get(id) {
        return this.client.request((httpClient, headers) => httpClient.get({
            url: "/api/dispatch-pools/{id}",
            headers,
            path: { id },
        }));
    }
    /**
     * Create a new dispatch pool.
     */
    create(data) {
        return this.client.request((httpClient, headers) => httpClient.post({
            url: "/api/dispatch-pools",
            headers: {
                ...headers,
                "Content-Type": "application/json",
            },
            body: data,
        }));
    }
    /**
     * Update a dispatch pool.
     */
    update(id, data) {
        return this.client.request((httpClient, headers) => httpClient.put({
            url: "/api/dispatch-pools/{id}",
            headers: {
                ...headers,
                "Content-Type": "application/json",
            },
            path: { id },
            body: data,
        }));
    }
    /**
     * Delete (archive) a dispatch pool.
     */
    delete(id) {
        return this.client.request((httpClient, headers) => httpClient.delete({
            url: "/api/dispatch-pools/{id}",
            headers,
            path: { id },
        }));
    }
    /**
     * Suspend a dispatch pool.
     */
    suspend(id) {
        return this.client.request((httpClient, headers) => httpClient.post({
            url: "/api/dispatch-pools/{id}/suspend",
            headers,
            path: { id },
        }));
    }
    /**
     * Activate a dispatch pool.
     */
    activate(id) {
        return this.client.request((httpClient, headers) => httpClient.post({
            url: "/api/dispatch-pools/{id}/activate",
            headers,
            path: { id },
        }));
    }
    /**
     * Sync dispatch pools for an application.
     */
    sync(applicationCode, pools, removeUnlisted = false) {
        return this.client.request((httpClient, headers) => httpClient.post({
            url: "/api/dispatch-pools/sync",
            headers: {
                ...headers,
                "Content-Type": "application/json",
            },
            body: { applicationCode, pools },
            query: { removeUnlisted },
        }));
    }
}
