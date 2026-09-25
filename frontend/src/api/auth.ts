import { useAuthStore, type User } from "@/stores/auth";
import router from "@/router";
import { getErrorMessage } from "@/utils/errors";
import { authFetch } from "./auth-fetch";
import type { TwoFactorMethod } from "./twofactor";

// Auth endpoints are at /auth/* (not /api/auth/*)
const AUTH_URL = "/auth";

interface LoginCredentials {
	email: string;
	password: string;
}

export interface LoginResponse {
	principalId: string;
	name: string;
	email: string;
	roles: string[];
	clientId: string | null;
	/** Effective permission codes; absent from a backend that predates them. */
	permissions?: string[];
	/** The account signs in through a federated identity provider. */
	ssoManaged?: boolean;
}

/**
 * The on-the-wire shape of `/auth/login` and the 2FA completion endpoints
 * (verify / enroll-confirm): an "ok" answer carries the principal (and the
 * session cookie); a pending answer carries a token and method list instead.
 */
export interface RawLoginResponse extends Partial<LoginResponse> {
	status?: "ok" | "mfa_required" | "enrollment_required";
	mfaToken?: string;
	enrollToken?: string;
	methods?: TwoFactorMethod[];
	allowedMethods?: TwoFactorMethod[];
	rememberDeviceAllowed?: boolean;
	/** Shown once, after an enrolment that completed the sign-in. */
	recoveryCodes?: string[];
}

/** What the login page branches on after a password submit. */
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
	/**
	 * Only alongside `authMethod: "internal"`: the account is a password
	 * account that has never had a password set. The login page offers
	 * "Create your password" (an emailed link) instead of a password prompt.
	 */
	passwordSetupRequired?: boolean;
}

export function mapLoginResponseToUser(response: LoginResponse): User {
	return {
		id: response.principalId,
		email: response.email,
		name: response.name,
		clientId: response.clientId,
		roles: response.roles,
		permissions: Array.isArray(response.permissions) ? response.permissions : null,
		ssoManaged: response.ssoManaged ?? false,
	};
}

export async function checkEmailDomain(
	email: string,
): Promise<DomainCheckResponse> {
	const response = await fetch(`${AUTH_URL}/check-domain`, {
		method: "POST",
		headers: { "Content-Type": "application/json" },
		body: JSON.stringify({ email }),
		credentials: "include",
	});

	if (!response.ok) {
		throw new Error("Failed to check email domain");
	}

	return response.json();
}

export async function checkSession(): Promise<boolean> {
	const authStore = useAuthStore();
	authStore.setLoading(true);

	try {
		const response = await fetch(`${AUTH_URL}/me`, {
			credentials: "include",
		});

		if (!response.ok) {
			authStore.clearAuth();
			return false;
		}

		const data: LoginResponse = await response.json();
		authStore.setUser(mapLoginResponseToUser(data));
		return true;
	} catch {
		authStore.clearAuth();
		return false;
	}
}

/**
 * Fill in the signed-in user's permissions from `/auth/me` when the response
 * that signed them in did not carry them. Best effort: on any failure the
 * user keeps `permissions: null` and the old admin-role rule applies.
 */
export async function loadPermissions(): Promise<void> {
	const authStore = useAuthStore();
	try {
		const response = await fetch(`${AUTH_URL}/me`, { credentials: "include" });
		if (!response.ok) return;
		const data: LoginResponse = await response.json();
		if (authStore.user && Array.isArray(data.permissions)) {
			authStore.user = { ...authStore.user, permissions: data.permissions };
		}
	} catch {
		// Keep the fallback.
	}
}

// OAUTH_FORWARD_FIELDS is the /oauth/authorize parameter set the SPA
// round-trips through a login.
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

/** Rebuild the `/oauth/authorize` URL from a parameter getter. */
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

// Post-auth redirect hand-off for flows with an interstitial (2FA
// enrolment after a password set): the confirm response carries a
// server-validated redirectUri, stashed here (NEVER read from the URL — that
// would be an open redirect) and consumed by redirectAfterLogin once the
// interstitial completes.
const POST_AUTH_REDIRECT_KEY = "fc.post_auth_redirect";

export function setPostAuthRedirect(uri: string): void {
	try {
		sessionStorage.setItem(POST_AUTH_REDIRECT_KEY, uri);
	} catch {
		// Storage unavailable: the user lands on the default page instead.
	}
}

function consumePostAuthRedirect(): string | null {
	try {
		const uri = sessionStorage.getItem(POST_AUTH_REDIRECT_KEY);
		if (uri !== null) sessionStorage.removeItem(POST_AUTH_REDIRECT_KEY);
		return uri;
	} catch {
		return null;
	}
}

/**
 * Record the signed-in user in the store (the session cookie is already set
 * server-side). Does NOT navigate — a caller that must show something first
 * (recovery codes) calls `redirectAfterLogin` afterwards.
 */
export async function setSessionUser(data: RawLoginResponse): Promise<void> {
	const authStore = useAuthStore();
	authStore.setUser(mapLoginResponseToUser(data as LoginResponse));
	if (!Array.isArray(data.permissions)) {
		await loadPermissions();
	}
}

/**
 * The post-login navigation: a stashed server-validated redirect, the OIDC
 * interaction, the OAuth authorize round-trip, or the dashboard.
 */
export async function redirectAfterLogin(): Promise<void> {
	const stashed = consumePostAuthRedirect();
	if (stashed) {
		window.location.href = stashed;
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
	await router.replace("/dashboard");
}

/**
 * A completed sign-in with no interstitial (the password and 2FA-verify
 * paths): set the user, then navigate.
 */
export async function applyLoginSuccess(data: RawLoginResponse): Promise<void> {
	await setSessionUser(data);
	await redirectAfterLogin();
}

/**
 * Password sign-in. Either completes (session set, navigation done) or
 * answers the pending two-factor step: a challenge (`mfa_required`) or a
 * forced enrolment (`enrollment_required`), with no session yet.
 */
export async function login(credentials: LoginCredentials): Promise<LoginResult> {
	const authStore = useAuthStore();
	authStore.setLoading(true);
	authStore.setError(null);

	try {
		const data = await authFetch<RawLoginResponse>("/login", {
			method: "POST",
			body: JSON.stringify(credentials),
		});

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

		await applyLoginSuccess(data);
		return { status: "ok" };
	} catch (error: unknown) {
		authStore.setLoading(false);
		authStore.setError(
			getErrorMessage(error, "Login failed. Please check your credentials."),
		);
		throw error;
	}
}

export async function logout(): Promise<void> {
	const authStore = useAuthStore();

	try {
		await fetch(`${AUTH_URL}/logout`, {
			method: "POST",
			credentials: "include",
		});
	} catch {
		// Ignore errors - clear local state anyway
	}

	authStore.clearAuth();
	// Use replace to clear navigation history on logout
	await router.replace("/auth/login");
}

/**
 * Email a reset link. `redirectUri`, when present, is the rebuilt
 * `/oauth/authorize?…` URL of the sign-in the user was in; the server keeps
 * it with the token (accepting only that shape) and returns it from confirm.
 */
export async function requestPasswordReset(
	email: string,
	redirectUri?: string,
): Promise<void> {
	await authFetch<unknown>("/password-reset/request", {
		method: "POST",
		body: JSON.stringify(redirectUri ? { email, redirectUri } : { email }),
	});
}

/**
 * Email a "create your password" link to a user who has never had one (see
 * `DomainCheckResponse.passwordSetupRequired`). A new password is never
 * accepted inline — the emailed link proves mailbox ownership. Always a
 * silent success. `redirectUri` must be a same-site relative URL (the
 * rebuilt `/oauth/authorize?…`), so the user returns to the application.
 */
export async function requestPasswordSetup(
	email: string,
	redirectUri?: string,
): Promise<{ message: string }> {
	return authFetch<{ message: string }>("/password-setup/request", {
		method: "POST",
		body: JSON.stringify(redirectUri ? { email, redirectUri } : { email }),
	});
}

export interface ValidateResetTokenResult {
	valid: boolean;
	reason?: string | null;
	/** The reset also needs a current authenticator (TOTP) code. */
	requiresFactor?: boolean;
}

export async function validateResetToken(
	token: string,
): Promise<ValidateResetTokenResult> {
	try {
		return await authFetch<ValidateResetTokenResult>(
			`/password-reset/validate?token=${encodeURIComponent(token)}`,
		);
	} catch {
		return { valid: false, reason: "not_found" };
	}
}

export interface ConfirmPasswordResetResult {
	status: "ok" | "enrollment_required";
	message: string;
	/** With `enrollment_required`: the domain requires 2FA, set it up now. */
	enrollToken?: string;
	allowedMethods?: TwoFactorMethod[];
	/** Server-validated post-set-password destination. Never from the URL. */
	redirectUri?: string | null;
	/**
	 * The server already set the session cookie (a completed invite with no
	 * 2FA required): go straight in rather than back to the login page.
	 */
	sessionEstablished?: boolean;
}

export async function confirmPasswordReset(
	token: string,
	password: string,
	factorCode?: string,
): Promise<ConfirmPasswordResetResult> {
	return authFetch<ConfirmPasswordResetResult>("/password-reset/confirm", {
		method: "POST",
		body: JSON.stringify(
			factorCode ? { token, password, factorCode } : { token, password },
		),
	});
}

export async function switchClient(clientId: string): Promise<void> {
	const authStore = useAuthStore();

	try {
		const response = await fetch(`${AUTH_URL}/client/${clientId}`, {
			method: "POST",
			credentials: "include",
		});

		if (!response.ok) {
			const errorData = await response.json().catch(() => ({}));
			throw new Error(errorData.message || "Failed to switch client");
		}

		authStore.selectClient(clientId);
	} catch (error: unknown) {
		authStore.setError(getErrorMessage(error, "Failed to switch client"));
		throw error;
	}
}
