import { apiFetch } from "./client";

export type PrincipalType = "USER" | "SERVICE";
export type IdpType = "INTERNAL" | "OIDC" | "SAML";
export type PrincipalScope = "ANCHOR" | "PARTNER" | "CLIENT";

export interface User {
	id: string;
	type: PrincipalType;
	scope: PrincipalScope | null;
	clientId: string | null;
	name: string;
	active: boolean;
	email: string | null;
	idpType: IdpType | null;
	roles: string[];
	isAnchorUser: boolean;
	grantedClientIds: string[];
	createdAt: string;
	updatedAt: string;
}

export interface UserListResponse {
	principals: User[];
	total: number;
}

export interface CreateUserRequest {
	email: string;
	password?: string; // Optional - only required for INTERNAL auth users
	name: string;
	clientId?: string;
}

export interface ClientAccessGrant {
	id: string;
	clientId: string;
	grantedAt: string;
	expiresAt: string | null;
}

export interface UpdateUserRequest {
	name: string;
	scope?: PrincipalScope;
	clientId?: string | null;
}

export interface RoleAssignment {
	id: string;
	roleName: string;
	assignmentSource: string;
	assignedAt: string;
}

export interface RolesAssignedResponse {
	roles: RoleAssignment[];
	added: string[];
	removed: string[];
}

export interface ApplicationAccessGrant {
	applicationId: string;
	applicationCode: string;
	applicationName: string;
}

export interface ApplicationAccessListResponse {
	applications: ApplicationAccessGrant[];
	total: number;
}

export interface ApplicationAccessAssignedResponse {
	applications: ApplicationAccessGrant[];
	added: number;
	removed: number;
}

export interface AvailableApplication {
	id: string;
	code: string;
	name: string;
}

export interface AvailableApplicationsResponse {
	applications: AvailableApplication[];
}

export interface EmailDomainCheckResponse {
	domain: string;
	authProvider: string;
	isAnchorDomain: boolean;
	hasIdpConfig: boolean;
	emailExists: boolean;
	info: string | null;
	warning: string | null;
}

export interface UserFilters {
	clientId?: string;
	type?: PrincipalType;
	active?: boolean;
	q?: string;
	roles?: string[];
	page?: number;
	pageSize?: number;
	sortField?: string;
	sortOrder?: string;
}

export const usersApi = {
	list(filters?: UserFilters): Promise<UserListResponse> {
		const params = new URLSearchParams();
		if (filters?.clientId) params.append("clientId", filters.clientId);
		if (filters?.type) params.append("type", filters.type);
		if (filters?.active !== undefined)
			params.append("active", String(filters.active));
		if (filters?.q) params.append("q", filters.q);
		if (filters?.roles?.length) params.append("roles", filters.roles.join(","));
		if (filters?.page !== undefined)
			params.append("page", String(filters.page));
		if (filters?.pageSize !== undefined)
			params.append("pageSize", String(filters.pageSize));
		if (filters?.sortField) params.append("sortField", filters.sortField);
		if (filters?.sortOrder) params.append("sortOrder", filters.sortOrder);

		const query = params.toString();
		return apiFetch(`/principals${query ? `?${query}` : ""}`);
	},

	get(id: string): Promise<User> {
		return apiFetch(`/principals/${id}`);
	},

	create(data: CreateUserRequest): Promise<User> {
		return apiFetch("/principals/users", {
			method: "POST",
			body: JSON.stringify(data),
		});
	},

	update(id: string, data: UpdateUserRequest): Promise<User> {
		return apiFetch(`/principals/${id}`, {
			method: "PUT",
			body: JSON.stringify(data),
		});
	},

	activate(id: string): Promise<{ message: string }> {
		return apiFetch(`/principals/${id}/activate`, {
			method: "POST",
		});
	},

	deactivate(id: string): Promise<{ message: string }> {
		return apiFetch(`/principals/${id}/deactivate`, {
			method: "POST",
		});
	},

	resetPassword(id: string, newPassword: string): Promise<{ message: string }> {
		return apiFetch(`/principals/${id}/reset-password`, {
			method: "POST",
			body: JSON.stringify({ newPassword }),
		});
	},

	/**
	 * Trigger a password reset email for an internal-auth user.
	 * Sends the same single-use email as the user-initiated /auth/password-reset/request flow.
	 * Rejects OIDC users and users without an email.
	 */
	sendPasswordReset(id: string): Promise<{ message: string }> {
		return apiFetch(`/principals/${id}/send-password-reset`, {
			method: "POST",
		});
	},

	// Client access grants
	getClientAccess(id: string): Promise<{ grants: ClientAccessGrant[] }> {
		return apiFetch(`/principals/${id}/client-access`);
	},

	grantClientAccess(id: string, clientId: string): Promise<ClientAccessGrant> {
		return apiFetch(`/principals/${id}/client-access`, {
			method: "POST",
			body: JSON.stringify({ clientId }),
		});
	},

	revokeClientAccess(id: string, clientId: string): Promise<void> {
		return apiFetch(`/principals/${id}/client-access/${clientId}`, {
			method: "DELETE",
		});
	},

	delete(id: string): Promise<void> {
		return apiFetch(`/principals/${id}`, {
			method: "DELETE",
		});
	},

	checkEmailDomain(email: string): Promise<EmailDomainCheckResponse> {
		return apiFetch(
			`/principals/check-email-domain?email=${encodeURIComponent(email)}`,
		);
	},

	// Role management
	getRoles(id: string): Promise<{ roles: RoleAssignment[] }> {
		return apiFetch(`/principals/${id}/roles`);
	},

	assignRole(id: string, roleName: string): Promise<RoleAssignment> {
		return apiFetch(`/principals/${id}/roles`, {
			method: "POST",
			body: JSON.stringify({ roleName }),
		});
	},

	removeRole(id: string, roleName: string): Promise<void> {
		return apiFetch(
			`/principals/${id}/roles/${encodeURIComponent(roleName)}`,
			{
				method: "DELETE",
			},
		);
	},

	/**
	 * Batch assign roles to a user.
	 * This is a declarative operation - sets the complete role list.
	 * Roles not in the list will be removed, new roles will be added.
	 */
	assignRoles(id: string, roles: string[]): Promise<RolesAssignedResponse> {
		return apiFetch(`/principals/${id}/roles`, {
			method: "PUT",
			body: JSON.stringify({ roles }),
		});
	},

	// Application access management

	/**
	 * Get the application access grants for a user.
	 */
	getApplicationAccess(id: string): Promise<ApplicationAccessListResponse> {
		return apiFetch(`/principals/${id}/application-access`);
	},

	/**
	 * Get applications available to grant to a user.
	 * Returns applications that are enabled for at least one of the user's accessible clients.
	 */
	getAvailableApplications(id: string): Promise<AvailableApplicationsResponse> {
		return apiFetch(`/principals/${id}/available-applications`);
	},

	/**
	 * Batch assign application access to a user.
	 * This is a declarative operation - sets the complete application access list.
	 * Applications not in the list will be removed, new applications will be added.
	 */
	assignApplicationAccess(
		id: string,
		applicationIds: string[],
	): Promise<ApplicationAccessAssignedResponse> {
		return apiFetch(`/principals/${id}/application-access`, {
			method: "PUT",
			body: JSON.stringify({ applicationIds }),
		});
	},
};
