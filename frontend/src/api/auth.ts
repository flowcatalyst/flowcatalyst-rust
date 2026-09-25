// Stays hand-rolled: auth surface — chi-mounted outside huma, not in the
// OpenAPI spec. Revisit if these routes converge on huma.
import { useAuthStore, type User } from "@/stores/auth";
import { landingPath } from "@/stores/permissions";
import router from "@/router";
import { getErrorMessage } from "@/utils/errors";
import { authFetch } from "./client";
import type { TwoFactorMethod } from "./twofactor";

interface LoginCredentials {
	email: string;
	password: string;
}

interface LoginResponse {
	principalId: string;
	name: string;
	email: string;
	roles: string[];
	permissions?: string[];
	clientId: string | null;
	ssoManaged?: boolean;
}

// RawLoginResponse is the on-the-wire shape of /auth/login and the 2FA
// completion endpoints (verify / enroll-confirm): an "ok" payload carries the
// principal; the pending statuses carry a token + method list instead.
export interface RawLoginResponse extends Partial<LoginResponse> {
	status?: "ok" | "mfa_required" | "enrollment_required";
	mfaToken?: string;
	enrollToken?: string;
	methods?: TwoFactorMethod[];
	allowedMethods?: TwoFactorMethod[];
	rememberDeviceAllowed?: boolean;
	recoveryCodes?: string[];
}

// LoginResult is what the SPA branches on after a password submit.
export type LoginResult =
	| { status: "ok" }
	| {
			status: "mfa_required";
			mfaToken: string;
			methods: TwoFactorMethod[];
			rememberDeviceAllowed: boolean;
	  }
	| {
			status: "enrollment_required";
			enrollToken: string;
			allowedMethods: TwoFactorMethod[];
	  };

export interface DomainCheckResponse {
	authMethod: "internal" | "external";
	loginUrl?: string;
	idpIssuer?: string;
	// Only ever set alongside authMethod:"internal": the account exists, is a
	// password account, and has never had a password set (e.g. an app-created
	// user whose invite email was suppressed). The login page must route to
	// the password-setup step instead of asking for a password.
	passwordSetupRequired?: boolean;
}

function mapLoginResponseToUser(response: LoginResponse): User {
	return {
		id: response.principalId,
		email: response.email,
		name: response.name,
		clientId: response.clientId,
		roles: response.roles,
		// Flat list of permission codes the backend resolved from the
		// user's roles. Empty when the backend doesn't ship them.
		permissions: response.permissions ?? [],
		ssoManaged: response.ssoManaged ?? false,
	};
}

export async function checkEmailDomain(
	email: string,
): Promise<DomainCheckResponse> {
	return authFetch<DomainCheckResponse>("/check-domain", {
		method: "POST",
		body: JSON.stringify({ email }),
	});
}

export async function checkSession(): Promise<boolean> {
	const authStore = useAuthStore();
	authStore.setLoading(true);

	try {
		const data = await authFetch<LoginResponse>("/me");
		authStore.setUser(mapLoginResponseToUser(data));
		return true;
	} catch {
		authStore.clearAuth();
		return false;
	}
}

// setSessionUser records the authenticated user in the store (the session
// cookie is already set server-side). It does NOT navigate — callers that need
// to show something first (e.g. recovery codes) call redirectAfterLogin later.
export function setSessionUser(data: RawLoginResponse): void {
	const authStore = useAuthStore();
	authStore.setUser(mapLoginResponseToUser(data as LoginResponse));
}

// OAUTH_FORWARD_FIELDS is the full /oauth/authorize parameter set the SPA
// round-trips through a login. One list, shared by every redirect path —
// the per-page copies used to drift.
const OAUTH_FORWARD_FIELDS = [
	"response_type",
	"client_id",
	"redirect_uri",
	"scope",
	"state",
	"code_challenge",
	"code_challenge_method",
	"nonce",
] as const;

// oauthAuthorizeUrl rebuilds the /oauth/authorize URL from a parameter
// getter (window URL params, or a router `to.query` adapter in guards).
export function oauthAuthorizeUrl(
	get: (field: string) => string | null,
): string {
	const params = new URLSearchParams();
	for (const field of OAUTH_FORWARD_FIELDS) {
		const value = get(field);
		if (value) params.set(field, value);
	}
	return `/oauth/authorize?${params.toString()}`;
}

// externalIdpRedirectUrl decorates an external IdP login URL
// (/auth/oidc/login) with the OIDC-interaction or OAuth round-trip context
// from the current page URL, so the bridge can resume the authorize flow
// after the IdP callback. The oauth_ prefix and the field list match
// exactly what the bridge's handleLogin consumes — response_type is
// intentionally absent (the bridge doesn't read oauth_response_type).
export function externalIdpRedirectUrl(loginUrl: string): string {
	const currentParams = new URLSearchParams(window.location.search);
	const url = new URL(loginUrl, window.location.origin);

	const interactionUid = currentParams.get("interaction");
	if (interactionUid) {
		url.searchParams.set("interaction", interactionUid);
		return url.toString();
	}
	if (currentParams.get("oauth") === "true") {
		const bridgeFields = [
			"client_id",
			"redirect_uri",
			"scope",
			"state",
			"code_challenge",
			"code_challenge_method",
			"nonce",
		];
		for (const field of bridgeFields) {
			const value = currentParams.get(field);
			if (value) url.searchParams.set("oauth_" + field, value);
		}
	}
	return url.toString();
}

// Post-auth redirect hand-off for flows with an interstitial (2FA
// enrollment): the reset-password confirm response carries a
// server-validated redirectUri, stashed here (NEVER read from the URL — that
// would be an open redirect) and consumed by redirectAfterLogin once the
// interstitial completes.
const POST_AUTH_REDIRECT_KEY = "fc.post_auth_redirect";

export function setPostAuthRedirect(uri: string): void {
	sessionStorage.setItem(POST_AUTH_REDIRECT_KEY, uri);
}

function consumePostAuthRedirect(): string | null {
	const uri = sessionStorage.getItem(POST_AUTH_REDIRECT_KEY);
	if (uri !== null) sessionStorage.removeItem(POST_AUTH_REDIRECT_KEY);
	return uri;
}

// redirectAfterLogin performs the post-login navigation: a stashed
// server-validated portal redirect, OIDC interaction, OAuth authorize
// round-trip, or the dashboard.
export function redirectAfterLogin(): void {
	const portalRedirect = consumePostAuthRedirect();
	if (portalRedirect) {
		window.location.href = portalRedirect;
		return;
	}
	const urlParams = new URLSearchParams(window.location.search);
	const interactionUid = urlParams.get("interaction");
	if (interactionUid) {
		window.location.href = `/oidc/interaction/${interactionUid}/login`;
		return;
	}
	if (urlParams.get("oauth") === "true") {
		window.location.href = oauthAuthorizeUrl((f) => urlParams.get(f));
		return;
	}
	// The most capable page the user can reach: dashboard (anchor/admin), else a
	// client-administrator's user-management page, else their profile.
	const authStore = useAuthStore();
	void router.replace(landingPath(authStore.user));
}

// applyLoginSuccess = set user + redirect. Used by the password and 2FA-verify
// paths (no interstitial). The enroll-and-complete path uses setSessionUser +
// redirectAfterLogin separately so it can show recovery codes in between.
export function applyLoginSuccess(data: RawLoginResponse): void {
	setSessionUser(data);
	redirectAfterLogin();
}

export async function login(
	credentials: LoginCredentials,
): Promise<LoginResult> {
	const authStore = useAuthStore();
	authStore.setLoading(true);
	authStore.setError(null);

	try {
		const data = await authFetch<RawLoginResponse>("/login", {
			method: "POST",
			body: JSON.stringify(credentials),
		});

		// 2FA pending: no session yet — hand the token back to the caller.
		if (data.status === "mfa_required") {
			authStore.setLoading(false);
			return {
				status: "mfa_required",
				mfaToken: data.mfaToken ?? "",
				methods: data.methods ?? [],
				rememberDeviceAllowed: data.rememberDeviceAllowed ?? false,
			};
		}
		if (data.status === "enrollment_required") {
			authStore.setLoading(false);
			return {
				status: "enrollment_required",
				enrollToken: data.enrollToken ?? "",
				allowedMethods: data.allowedMethods ?? [],
			};
		}

		applyLoginSuccess(data);
		return { status: "ok" };
	} catch (error: unknown) {
		authStore.setLoading(false);
		authStore.setError(getErrorMessage(error, "Login failed"));
		throw error;
	}
}

export async function logout(): Promise<void> {
	const authStore = useAuthStore();

	try {
		await authFetch<void>("/logout", { method: "POST" });
	} catch {
		// Ignore errors - clear local state anyway
	}

	authStore.clearAuth();
	// Use replace to clear navigation history on logout
	await router.replace("/auth/login");
}

// requestPasswordReset emails a reset link. redirectUri, when present, is the
// rebuilt /oauth/authorize?… URL of the sign-in the user was in the middle
// of; the server stores it with the token (accepting only that shape) and
// returns it from confirm, so the reset page can resume the round-trip.
export async function requestPasswordReset(email: string, redirectUri?: string): Promise<void> {
	await authFetch<void>("/password-reset/request", {
		method: "POST",
		body: JSON.stringify(redirectUri ? { email, redirectUri } : { email }),
	});
}

// requestPasswordSetup emails a set-password link to a user who has never
// had a password (see DomainCheckResponse.passwordSetupRequired). We never
// accept a new password inline here — the emailed link is what proves
// mailbox ownership. Always resolves 200 (silent success; never reveals
// whether the account exists). redirectUri, when present, must be a
// same-site relative URL (the rebuilt /oauth/authorize?... URL) so the
// server can hand the user back to the calling application once they've set
// their password.
export async function requestPasswordSetup(
	email: string,
	redirectUri?: string,
): Promise<{ message: string }> {
	return authFetch<{ message: string }>("/password-setup/request", {
		method: "POST",
		body: JSON.stringify({ email, redirectUri }),
	});
}

export async function validateResetToken(
	token: string,
): Promise<{
	valid: boolean;
	reason?: string;
	requiresFactor?: boolean;
	// Portal-identity token: the page shows portal framing (no platform brand).
	portal?: boolean;
}> {
	try {
		return await authFetch(
			`/password-reset/validate?token=${encodeURIComponent(token)}`,
		);
	} catch {
		return { valid: false, reason: "not_found" };
	}
}

export interface ConfirmPasswordResetResult {
	status: "ok" | "enrollment_required";
	message: string;
	enrollToken?: string;
	allowedMethods?: TwoFactorMethod[];
	// Server-validated post-set-password destination (portal invites chain
	// back into the portal's OAuth login). Never derived from the URL.
	redirectUri?: string | null;
	// Marks a PORTAL-identity confirm: with no redirectUri the page must not
	// bounce to the platform login (it cannot sign portal users in).
	portal?: boolean;
	// True when the server has already set the fc_session cookie as part of
	// this confirm (invite flows only, and only when no 2FA is required).
	// The page is signed in and should go straight to the landing page
	// rather than bouncing to /auth/login.
	sessionEstablished?: boolean;
}

export async function confirmPasswordReset(
	token: string,
	password: string,
	factorCode?: string,
): Promise<ConfirmPasswordResetResult> {
	return authFetch<ConfirmPasswordResetResult>("/password-reset/confirm", {
		method: "POST",
		body: JSON.stringify({ token, password, factorCode }),
	});
}

export async function switchClient(clientId: string): Promise<void> {
	const authStore = useAuthStore();

	try {
		// POST /auth/client/switch {clientId} — the backend has no
		// /auth/client/{id} route (this used to 404/405).
		await authFetch<void>("/client/switch", {
			method: "POST",
			body: JSON.stringify({ clientId }),
		});
		authStore.selectClient(clientId);
	} catch (error: unknown) {
		authStore.setError(getErrorMessage(error, "Failed to switch client"));
		throw error;
	}
}
