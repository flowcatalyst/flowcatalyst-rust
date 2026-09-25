/**
 * Principals Resource
 *
 * Manage users and service accounts.
 */

import type { ResultAsync } from "neverthrow";
import type { SdkError } from "../errors.js";
import type { FlowCatalystClient } from "../client.js";
import * as sdk from "../generated/sdk.gen.js";
import type {
	ListPrincipalsData,
	ListPrincipalsResponse,
	GetPrincipalResponse,
	CreateUserData,
	UpdatePrincipalData,
	ResetPrincipalPasswordData,
	ListPrincipalRolesResponse,
	ListPrincipalClientAccessResponse,
	SyncPrincipalsData,
	SyncPrincipalsResponse as SyncPrincipalsResponseType,
	SyncUsersData,
	SyncUsersResponse as SyncUsersResponseType,
} from "../generated/types.gen.js";

export type PrincipalListResponse = ListPrincipalsResponse;
export type PrincipalDto = GetPrincipalResponse;
export type CreateUserRequest = CreateUserData["body"];
export type UpdatePrincipalRequest = UpdatePrincipalData["body"];
export type ResetPasswordRequest = ResetPrincipalPasswordData["body"];
export type RoleListResponse = ListPrincipalRolesResponse;
export type ClientAccessListResponse = ListPrincipalClientAccessResponse;
export type SyncPrincipalsResponse = SyncPrincipalsResponseType;
export type SyncUsersResponse = SyncUsersResponseType;

// Derived from the generated query type so it stays in sync with the platform
// spec automatically — adding a query param upstream surfaces here on regen.
export type PrincipalFilters = ListPrincipalsData["query"];

/**
 * Principals resource for managing users and service accounts.
 */
export class PrincipalsResource {
	private readonly client: FlowCatalystClient;

	constructor(client: FlowCatalystClient) {
		this.client = client;
	}

	/**
	 * List all principals with optional filters.
	 */
	list(
		filters?: PrincipalFilters,
	): ResultAsync<PrincipalListResponse, SdkError> {
		return this.client.request<PrincipalListResponse>((httpClient, headers) =>
			sdk.listPrincipals({
				client: httpClient,
				headers,
				query: filters,
			}),
		);
	}

	/**
	 * Get a principal by ID.
	 */
	get(id: string): ResultAsync<PrincipalDto, SdkError> {
		return this.client.request<PrincipalDto>((httpClient, headers) =>
			sdk.getPrincipal({
				client: httpClient,
				headers,
				path: { id },
			}),
		);
	}

	/**
	 * Find a user by email.
	 *
	 * Client-side filters the response to rows whose email matches exactly
	 * (case-insensitive). Older platform builds silently ignored unknown
	 * query parameters and returned an unfiltered list; we defend against
	 * that here so callers don't act on the wrong principal.
	 */
	findByEmail(email: string): ResultAsync<PrincipalListResponse, SdkError> {
		const needle = email.toLowerCase();
		return this.list({ q: email }).map((response) => {
			const principals = response.principals.filter(
				(p) => (p.email ?? "").toLowerCase() === needle,
			);
			return { principals, total: principals.length };
		});
	}

	/**
	 * Create a new user principal.
	 *
	 * `sendInvitation` (default true) controls whether the platform emails
	 * the new user at all: a passwordless user gets the "set your password"
	 * invite, a user created with a password gets the "account created"
	 * welcome. Set to `false` when the calling application is taking over
	 * the invitation itself — this suppresses BOTH emails.
	 *
	 * `returnInviteLink` (default false), when true, mints the 72-hour
	 * "set your password" link and returns it as `inviteLink` on the
	 * response instead of emailing it — only for a passwordless INTERNAL
	 * user (a no-op, field absent, otherwise). `returnInviteLink: true`
	 * ALWAYS suppresses the platform's own invite email, even when
	 * `sendInvitation` is true or unset — the token can only be minted
	 * once, so asking for the link back implies you're sending your own
	 * email with it.
	 *
	 * `inviteLink` is a live 72-hour bearer credential — treat it exactly
	 * like a password and never log it.
	 *
	 * `inviteRedirectUri` sends the invitee to your application once they
	 * have set their password (and enrolled 2FA, if their domain requires
	 * it) — any absolute http(s) URL, typically your app's own page, which
	 * then starts sign-in as usual. It applies to both the platform-sent
	 * invite email and `returnInviteLink`. A malformed value fails with
	 * `INVITE_REDIRECT_URI_INVALID` and no user is created.
	 */
	createUser(data: CreateUserRequest): ResultAsync<PrincipalDto, SdkError> {
		return this.client.request<PrincipalDto>((httpClient, headers) =>
			sdk.createUser({
				client: httpClient,
				headers,
				body: data,
			}),
		);
	}

	/**
	 * Update a principal.
	 */
	update(
		id: string,
		data: UpdatePrincipalRequest,
	): ResultAsync<PrincipalDto, SdkError> {
		return this.client.request<PrincipalDto>((httpClient, headers) =>
			sdk.updatePrincipal({
				client: httpClient,
				headers,
				path: { id },
				body: data,
			}),
		);
	}

	/**
	 * Activate a principal.
	 */
	activate(id: string): ResultAsync<PrincipalDto, SdkError> {
		return this.client.request<PrincipalDto>((httpClient, headers) =>
			sdk.activatePrincipal({
				client: httpClient,
				headers,
				path: { id },
			}),
		);
	}

	/**
	 * Deactivate a principal.
	 */
	deactivate(id: string): ResultAsync<PrincipalDto, SdkError> {
		return this.client.request<PrincipalDto>((httpClient, headers) =>
			sdk.deactivatePrincipal({
				client: httpClient,
				headers,
				path: { id },
			}),
		);
	}

	/**
	 * Reset a user's password.
	 *
	 * Set `enforcePasswordComplexity` on `data` to `false` when the caller
	 * enforces its own password policy; only the platform's 2-character
	 * minimum will apply. Defaults to `true`.
	 */
	resetPassword(
		id: string,
		data: ResetPasswordRequest,
	): ResultAsync<unknown, SdkError> {
		return this.client.request<unknown>((httpClient, headers) =>
			sdk.resetPrincipalPassword({
				client: httpClient,
				headers,
				path: { id },
				body: data,
			}),
		);
	}

	/**
	 * Get roles assigned to a principal.
	 */
	getRoles(id: string): ResultAsync<RoleListResponse, SdkError> {
		return this.client.request<RoleListResponse>((httpClient, headers) =>
			sdk.listPrincipalRoles({
				client: httpClient,
				headers,
				path: { id },
			}),
		);
	}

	/**
	 * Add a single role to a principal (additive — keeps existing roles).
	 *
	 * Renamed from `assignRole` to make the additive-vs-replace distinction
	 * visible at the call site (paired with `setRoles` for replace-all).
	 */
	addRole(id: string, roleName: string): ResultAsync<unknown, SdkError> {
		return this.client.request<unknown>((httpClient, headers) =>
			sdk.addPrincipalRole({
				client: httpClient,
				headers,
				path: { id },
				body: { role: roleName },
			}),
		);
	}

	/**
	 * Remove a role from a principal.
	 */
	removeRole(id: string, roleName: string): ResultAsync<unknown, SdkError> {
		return this.client.request<unknown>((httpClient, headers) =>
			sdk.removePrincipalRole({
				client: httpClient,
				headers,
				path: { id, role: roleName },
			}),
		);
	}

	/**
	 * Replace all roles on a principal with the given set (declarative).
	 *
	 * Renamed from `assignRoles` so the replace semantics are obvious
	 * (paired with `addRole` for additive).
	 */
	setRoles(id: string, roles: string[]): ResultAsync<unknown, SdkError> {
		return this.client.request<unknown>((httpClient, headers) =>
			sdk.assignPrincipalRoles({
				client: httpClient,
				headers,
				path: { id },
				body: { roles },
			}),
		);
	}

	/**
	 * Get client access grants for a principal.
	 */
	getClientAccessGrants(
		id: string,
	): ResultAsync<ClientAccessListResponse, SdkError> {
		return this.client.request<ClientAccessListResponse>(
			(httpClient, headers) =>
				sdk.listPrincipalClientAccess({
					client: httpClient,
					headers,
					path: { id },
				}),
		);
	}

	/**
	 * Grant client access to a principal.
	 */
	grantClientAccess(
		id: string,
		clientId: string,
	): ResultAsync<unknown, SdkError> {
		return this.client.request<unknown>((httpClient, headers) =>
			sdk.grantPrincipalClientAccess({
				client: httpClient,
				headers,
				path: { id },
				body: { clientId },
			}),
		);
	}

	/**
	 * Revoke client access from a principal.
	 */
	revokeClientAccess(
		id: string,
		clientId: string,
	): ResultAsync<unknown, SdkError> {
		return this.client.request<unknown>((httpClient, headers) =>
			sdk.revokePrincipalClientAccess({
				client: httpClient,
				headers,
				path: { id, clientId },
			}),
		);
	}

	/**
	 * Sync principals for an application — declarative reconciliation
	 * against `POST /api/applications/{applicationCode}/principals/sync`.
	 *
	 * When `removeUnlisted` is true the platform strips SDK-sourced role
	 * assignments from principals not in the list; principals themselves
	 * are never deleted by sync.
	 */
	sync(
		applicationCode: string,
		principals: SyncPrincipalsData["body"]["principals"],
		removeUnlisted = false,
	): ResultAsync<SyncPrincipalsResponse, SdkError> {
		return this.client.request<SyncPrincipalsResponse>(
			(httpClient, headers) =>
				sdk.syncPrincipals({
					client: httpClient,
					headers,
					path: { appCode: applicationCode },
					body: { principals },
					query: { removeUnlisted },
				}),
		);
	}

	/**
	 * Sync users platform-wide — declarative upsert keyed on email, with NO
	 * application scope (`POST /api/principals/sync`).
	 *
	 * The application-less twin of {@link sync}: use it to "just sync users" —
	 * migrating accounts and (via each entry's `passwordHash`) their existing
	 * password hashes — without nesting the call under an application. Users are
	 * global, matched by email, so an application code adds nothing here.
	 *
	 * Pure upsert: roles are never stripped from unlisted users.
	 */
	syncUsers(
		principals: SyncUsersData["body"]["principals"],
	): ResultAsync<SyncUsersResponse, SdkError> {
		return this.client.request<SyncUsersResponse>(
			(httpClient, headers) =>
				sdk.syncUsers({
					client: httpClient,
					headers,
					body: { principals },
				}),
		);
	}
}
