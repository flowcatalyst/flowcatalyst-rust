import { useAuthStore, type User } from "@/stores/auth";
import router from "@/router";
import { getErrorMessage } from "@/utils/errors";

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
}

export interface DomainCheckResponse {
	authMethod: "internal" | "external";
	loginUrl?: string;
	idpIssuer?: string;
}

export function mapLoginResponseToUser(response: LoginResponse): User {
	return {
		id: response.principalId,
		email: response.email,
		name: response.name,
		clientId: response.clientId,
		roles: response.roles,
		permissions: Array.isArray(response.permissions) ? response.permissions : null,
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

export async function login(credentials: LoginCredentials): Promise<void> {
	const authStore = useAuthStore();
	authStore.setLoading(true);
	authStore.setError(null);

	try {
		const response = await fetch(`${AUTH_URL}/login`, {
			method: "POST",
			headers: { "Content-Type": "application/json" },
			body: JSON.stringify(credentials),
			credentials: "include",
		});

		if (!response.ok) {
			const errorData = await response.json().catch(() => ({}));
			throw new Error(
				errorData.error || "Login failed. Please check your credentials.",
			);
		}

		const data: LoginResponse = await response.json();
		authStore.setUser(mapLoginResponseToUser(data));
		if (!Array.isArray(data.permissions)) {
			await loadPermissions();
		}

		// Check if this is part of an OIDC interaction flow
		const urlParams = new URLSearchParams(window.location.search);
		const interactionUid = urlParams.get("interaction");
		if (interactionUid) {
			window.location.href = `/oidc/interaction/${interactionUid}/login`;
			return;
		}

		// Check if this is part of an OAuth flow - redirect back to /oauth/authorize
		if (urlParams.get("oauth") === "true") {
			// Rebuild OAuth authorize URL with all params
			const oauthParams = new URLSearchParams();
			const oauthFields = [
				"response_type",
				"client_id",
				"redirect_uri",
				"scope",
				"state",
				"code_challenge",
				"code_challenge_method",
				"nonce",
			];
			for (const field of oauthFields) {
				const value = urlParams.get(field);
				if (value) oauthParams.set(field, value);
			}
			window.location.href = `/oauth/authorize?${oauthParams.toString()}`;
			return;
		}

		// Normal login - go to dashboard
		await router.replace("/dashboard");
	} catch (error: unknown) {
		authStore.setLoading(false);
		authStore.setError(getErrorMessage(error, "Login failed"));
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

export async function requestPasswordReset(email: string): Promise<void> {
	const response = await fetch(`${AUTH_URL}/password-reset/request`, {
		method: "POST",
		headers: { "Content-Type": "application/json" },
		body: JSON.stringify({ email }),
		credentials: "include",
	});

	if (!response.ok) {
		const errorData = await response.json().catch(() => ({}));
		throw new Error(
			errorData.error || "Failed to request password reset.",
		);
	}
}

export async function validateResetToken(
	token: string,
): Promise<{ valid: boolean; reason?: string }> {
	const response = await fetch(
		`${AUTH_URL}/password-reset/validate?token=${encodeURIComponent(token)}`,
		{ credentials: "include" },
	);

	if (!response.ok) {
		return { valid: false, reason: "not_found" };
	}

	return response.json();
}

export async function confirmPasswordReset(
	token: string,
	password: string,
): Promise<void> {
	const response = await fetch(`${AUTH_URL}/password-reset/confirm`, {
		method: "POST",
		headers: { "Content-Type": "application/json" },
		body: JSON.stringify({ token, password }),
		credentials: "include",
	});

	if (!response.ok) {
		const errorData = await response.json().catch(() => ({}));
		throw new Error(
			errorData.error || "Failed to reset password.",
		);
	}
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
