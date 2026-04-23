/**
 * Principals Resource
 *
 * Manage users and service accounts.
 */
import * as sdk from "../generated/sdk.gen";
/**
 * Principals resource for managing users and service accounts.
 */
export class PrincipalsResource {
    constructor(client) {
        this.client = client;
    }
    /**
     * List all principals with optional filters.
     */
    list(filters) {
        return this.client.request((httpClient, headers) => sdk.getApiAdminPrincipals({
            client: httpClient,
            headers,
            query: filters,
        }));
    }
    /**
     * Get a principal by ID.
     */
    get(id) {
        return this.client.request((httpClient, headers) => sdk.getApiAdminPrincipalsById({
            client: httpClient,
            headers,
            path: { id },
        }));
    }
    /**
     * Find a user by email.
     *
     * Client-side filters the response to rows whose email matches exactly
     * (case-insensitive). Older platform builds silently ignored unknown
     * query parameters and returned an unfiltered list; we defend against
     * that here so callers don't act on the wrong principal.
     */
    findByEmail(email) {
        const needle = email.toLowerCase();
        return this.list({ email }).map((response) => {
            const principals = response.principals.filter((p) => (p.email ?? "").toLowerCase() === needle);
            return { principals, total: principals.length };
        });
    }
    /**
     * Create a new user principal.
     */
    createUser(data) {
        return this.client.request((httpClient, headers) => sdk.postApiAdminPrincipalsUsers({
            client: httpClient,
            headers,
            body: data,
        }));
    }
    /**
     * Update a principal.
     */
    update(id, data) {
        return this.client.request((httpClient, headers) => sdk.putApiAdminPrincipalsById({
            client: httpClient,
            headers,
            path: { id },
            body: data,
        }));
    }
    /**
     * Activate a principal.
     */
    activate(id) {
        return this.client.request((httpClient, headers) => sdk.postApiAdminPrincipalsByIdActivate({
            client: httpClient,
            headers,
            path: { id },
        }));
    }
    /**
     * Deactivate a principal.
     */
    deactivate(id) {
        return this.client.request((httpClient, headers) => sdk.postApiAdminPrincipalsByIdDeactivate({
            client: httpClient,
            headers,
            path: { id },
        }));
    }
    /**
     * Reset a user's password.
     *
     * Set `enforcePasswordComplexity` on `data` to `false` when the caller
     * enforces its own password policy; only the platform's 2-character
     * minimum will apply. Defaults to `true`.
     */
    resetPassword(id, data) {
        return this.client.request((httpClient, headers) => sdk.postApiAdminPrincipalsByIdResetPassword({
            client: httpClient,
            headers,
            path: { id },
            body: data,
        }));
    }
    /**
     * Get roles assigned to a principal.
     */
    getRoles(id) {
        return this.client.request((httpClient, headers) => sdk.getApiAdminPrincipalsByIdRoles({
            client: httpClient,
            headers,
            path: { id },
        }));
    }
    /**
     * Assign a single role to a principal.
     */
    assignRole(id, roleName) {
        return this.client.request((httpClient, headers) => sdk.postApiAdminPrincipalsByIdRoles({
            client: httpClient,
            headers,
            path: { id },
            body: { role: roleName },
        }));
    }
    /**
     * Remove a role from a principal.
     */
    removeRole(id, roleName) {
        return this.client.request((httpClient, headers) => sdk.deleteApiAdminPrincipalsByIdRolesByRoleName({
            client: httpClient,
            headers,
            path: { id, role: roleName },
        }));
    }
    /**
     * Assign roles to a principal (declarative - replaces all roles).
     */
    assignRoles(id, roles) {
        return this.client.request((httpClient, headers) => sdk.putApiAdminPrincipalsByIdRoles({
            client: httpClient,
            headers,
            path: { id },
            body: { roles },
        }));
    }
    /**
     * Get client access grants for a principal.
     */
    getClientAccessGrants(id) {
        return this.client.request((httpClient, headers) => sdk.getApiAdminPrincipalsByIdClientAccess({
            client: httpClient,
            headers,
            path: { id },
        }));
    }
    /**
     * Grant client access to a principal.
     */
    grantClientAccess(id, clientId) {
        return this.client.request((httpClient, headers) => sdk.postApiAdminPrincipalsByIdClientAccess({
            client: httpClient,
            headers,
            path: { id },
            body: { clientId },
        }));
    }
    /**
     * Revoke client access from a principal.
     */
    revokeClientAccess(id, clientId) {
        return this.client.request((httpClient, headers) => sdk.deleteApiAdminPrincipalsByIdClientAccessByClientId({
            client: httpClient,
            headers,
            path: { id, client_id: clientId },
        }));
    }
}
