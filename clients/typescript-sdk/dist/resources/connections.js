/**
 * Connections Resource
 *
 * Manage connections between service accounts and subscription targets.
 *
 * Uses direct HTTP calls since generated SDK functions are not yet available
 * (OpenAPI spec not regenerated). Will be migrated to generated functions
 * once the spec is updated.
 */
/**
 * Connections resource for managing service-account-to-target connections.
 */
export class ConnectionsResource {
    constructor(client) {
        this.client = client;
    }
    /**
     * List all connections with optional filters.
     */
    list(filters) {
        return this.client.request((httpClient, headers) => httpClient.get({
            url: "/api/connections",
            headers,
            query: filters,
        }));
    }
    /**
     * Get a connection by ID.
     */
    get(id) {
        return this.client.request((httpClient, headers) => httpClient.get({
            url: "/api/connections/{id}",
            headers,
            path: { id },
        }));
    }
    /**
     * Create a new connection.
     */
    create(data) {
        return this.client.request((httpClient, headers) => httpClient.post({
            url: "/api/connections",
            headers: {
                ...headers,
                "Content-Type": "application/json",
            },
            body: data,
        }));
    }
    /**
     * Update a connection.
     */
    update(id, data) {
        return this.client.request((httpClient, headers) => httpClient.put({
            url: "/api/connections/{id}",
            headers: {
                ...headers,
                "Content-Type": "application/json",
            },
            path: { id },
            body: data,
        }));
    }
    /**
     * Delete a connection.
     */
    delete(id) {
        return this.client.request((httpClient, headers) => httpClient.delete({
            url: "/api/connections/{id}",
            headers,
            path: { id },
        }));
    }
    /**
     * Pause a connection.
     */
    pause(id) {
        return this.client.request((httpClient, headers) => httpClient.post({
            url: "/api/connections/{id}/pause",
            headers,
            path: { id },
        }));
    }
    /**
     * Activate a connection.
     */
    activate(id) {
        return this.client.request((httpClient, headers) => httpClient.post({
            url: "/api/connections/{id}/activate",
            headers,
            path: { id },
        }));
    }
}
