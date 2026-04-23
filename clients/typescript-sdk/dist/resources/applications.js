/**
 * Applications Resource
 *
 * Manage applications in the platform.
 *
 * Uses direct HTTP calls since generated SDK functions are not yet available
 * (OpenAPI spec does not include /api/applications routes). Will be
 * migrated to generated functions once the spec is updated.
 */
/**
 * Applications resource for managing platform applications.
 */
export class ApplicationsResource {
    constructor(client) {
        this.client = client;
    }
    /**
     * List all applications.
     */
    list() {
        return this.client.request((httpClient, headers) => httpClient.get({
            url: "/api/applications",
            headers,
        }));
    }
    /**
     * Get an application by ID.
     */
    get(id) {
        return this.client.request((httpClient, headers) => httpClient.get({
            url: "/api/applications/{id}",
            headers,
            path: { id },
        }));
    }
    /**
     * Get an application by code.
     */
    getByCode(code) {
        return this.client.request((httpClient, headers) => httpClient.get({
            url: "/api/applications/by-code/{code}",
            headers,
            path: { code },
        }));
    }
    /**
     * Create a new application.
     */
    create(data) {
        return this.client.request((httpClient, headers) => httpClient.post({
            url: "/api/applications",
            headers: {
                ...headers,
                "Content-Type": "application/json",
            },
            body: data,
        }));
    }
    /**
     * Update an application.
     */
    update(id, data) {
        return this.client.request((httpClient, headers) => httpClient.put({
            url: "/api/applications/{id}",
            headers: {
                ...headers,
                "Content-Type": "application/json",
            },
            path: { id },
            body: data,
        }));
    }
    /**
     * Delete an application.
     */
    delete(id) {
        return this.client.request((httpClient, headers) => httpClient.delete({
            url: "/api/applications/{id}",
            headers,
            path: { id },
        }));
    }
    /**
     * Activate an application.
     */
    activate(id) {
        return this.client.request((httpClient, headers) => httpClient.post({
            url: "/api/applications/{id}/activate",
            headers,
            path: { id },
        }));
    }
    /**
     * Deactivate an application.
     */
    deactivate(id) {
        return this.client.request((httpClient, headers) => httpClient.post({
            url: "/api/applications/{id}/deactivate",
            headers,
            path: { id },
        }));
    }
    /**
     * Provision a service account for an application.
     */
    provisionServiceAccount(id) {
        return this.client.request((httpClient, headers) => httpClient.post({
            url: "/api/applications/{id}/provision-service-account",
            headers,
            path: { id },
        }));
    }
    /**
     * Get the service account attached to an application.
     */
    getServiceAccount(id) {
        return this.client.request((httpClient, headers) => httpClient.get({
            url: "/api/applications/{id}/service-account",
            headers,
            path: { id },
        }));
    }
    /**
     * List roles defined for an application.
     */
    listRoles(id) {
        return this.client.request((httpClient, headers) => httpClient.get({
            url: "/api/applications/{id}/roles",
            headers,
            path: { id },
        }));
    }
    /**
     * List per-client configs for an application.
     */
    listClients(id) {
        return this.client.request((httpClient, headers) => httpClient.get({
            url: "/api/applications/{id}/clients",
            headers,
            path: { id },
        }));
    }
    /**
     * Update the per-client config for an application.
     */
    updateClientConfig(id, clientId, data) {
        return this.client.request((httpClient, headers) => httpClient.put({
            url: "/api/applications/{id}/clients/{clientId}",
            headers: {
                ...headers,
                "Content-Type": "application/json",
            },
            path: { id, clientId },
            body: data,
        }));
    }
    /**
     * Enable an application for a specific client.
     */
    enableForClient(id, clientId) {
        return this.client.request((httpClient, headers) => httpClient.post({
            url: "/api/applications/{id}/clients/{clientId}/enable",
            headers,
            path: { id, clientId },
        }));
    }
    /**
     * Disable an application for a specific client.
     */
    disableForClient(id, clientId) {
        return this.client.request((httpClient, headers) => httpClient.post({
            url: "/api/applications/{id}/clients/{clientId}/disable",
            headers,
            path: { id, clientId },
        }));
    }
}
