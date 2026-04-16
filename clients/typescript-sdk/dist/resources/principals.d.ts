/**
 * Principals Resource
 *
 * Manage users and service accounts.
 */
import type { ResultAsync } from "neverthrow";
import type { SdkError } from "../errors";
import type { FlowCatalystClient } from "../client";
import type { GetApiAdminPrincipalsResponse, GetApiAdminPrincipalsByIdResponse, PostApiAdminPrincipalsUsersData, PutApiAdminPrincipalsByIdData, PostApiAdminPrincipalsByIdResetPasswordData, GetApiAdminPrincipalsByIdRolesResponse, GetApiAdminPrincipalsByIdClientAccessResponse } from "../generated/types.gen";
export type PrincipalListResponse = GetApiAdminPrincipalsResponse;
export type PrincipalDto = GetApiAdminPrincipalsByIdResponse;
export type CreateUserRequest = PostApiAdminPrincipalsUsersData["body"];
export type UpdatePrincipalRequest = PutApiAdminPrincipalsByIdData["body"];
export type ResetPasswordRequest = PostApiAdminPrincipalsByIdResetPasswordData["body"];
export type RoleListResponse = GetApiAdminPrincipalsByIdRolesResponse;
export type ClientAccessListResponse = GetApiAdminPrincipalsByIdClientAccessResponse;
export interface PrincipalFilters {
    clientId?: string;
    type?: string;
    active?: string;
    email?: string;
}
/**
 * Principals resource for managing users and service accounts.
 */
export declare class PrincipalsResource {
    private readonly client;
    constructor(client: FlowCatalystClient);
    /**
     * List all principals with optional filters.
     */
    list(filters?: PrincipalFilters): ResultAsync<PrincipalListResponse, SdkError>;
    /**
     * Get a principal by ID.
     */
    get(id: string): ResultAsync<PrincipalDto, SdkError>;
    /**
     * Find a user by email.
     */
    findByEmail(email: string): ResultAsync<PrincipalListResponse, SdkError>;
    /**
     * Create a new user principal.
     */
    createUser(data: CreateUserRequest): ResultAsync<PrincipalDto, SdkError>;
    /**
     * Update a principal.
     */
    update(id: string, data: UpdatePrincipalRequest): ResultAsync<PrincipalDto, SdkError>;
    /**
     * Activate a principal.
     */
    activate(id: string): ResultAsync<PrincipalDto, SdkError>;
    /**
     * Deactivate a principal.
     */
    deactivate(id: string): ResultAsync<PrincipalDto, SdkError>;
    /**
     * Reset a user's password.
     */
    resetPassword(id: string, data: ResetPasswordRequest): ResultAsync<unknown, SdkError>;
    /**
     * Get roles assigned to a principal.
     */
    getRoles(id: string): ResultAsync<RoleListResponse, SdkError>;
    /**
     * Assign a single role to a principal.
     */
    assignRole(id: string, roleName: string): ResultAsync<unknown, SdkError>;
    /**
     * Remove a role from a principal.
     */
    removeRole(id: string, roleName: string): ResultAsync<unknown, SdkError>;
    /**
     * Assign roles to a principal (declarative - replaces all roles).
     */
    assignRoles(id: string, roles: string[]): ResultAsync<unknown, SdkError>;
    /**
     * Get client access grants for a principal.
     */
    getClientAccessGrants(id: string): ResultAsync<ClientAccessListResponse, SdkError>;
    /**
     * Grant client access to a principal.
     */
    grantClientAccess(id: string, clientId: string): ResultAsync<unknown, SdkError>;
    /**
     * Revoke client access from a principal.
     */
    revokeClientAccess(id: string, clientId: string): ResultAsync<unknown, SdkError>;
}
//# sourceMappingURL=principals.d.ts.map