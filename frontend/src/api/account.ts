// Account self-service on the auth surface (session-gated, /auth/*): change
// password (which may answer MFA_REQUIRED until a second-factor code is
// given) and the caller's recent sign-in history.
import { ApiError } from "./client";
import { authFetch } from "./auth-fetch";

export interface ChangePasswordResult {
	ok: boolean;
	/** A code from a confirmed second factor is needed to go on. */
	mfaRequired?: boolean;
	/** Confirmed factor types, e.g. ["TOTP", "EMAIL_PIN"]. */
	methods?: string[];
	errorCode?: string;
	message?: string;
}

/**
 * `POST /auth/change-password`. Resolves with the outcome rather than
 * throwing on a rejected change, so the dialog can reveal its 2FA step.
 */
export async function changePassword(input: {
	currentPassword: string;
	newPassword: string;
	code?: string;
}): Promise<ChangePasswordResult> {
	try {
		const data = await authFetch<{ message?: string }>("/change-password", {
			method: "POST",
			body: JSON.stringify(input),
		});
		return { ok: true, message: data?.message };
	} catch (e) {
		if (!(e instanceof ApiError)) throw e;
		const methods = e.details?.["methods"];
		return {
			ok: false,
			mfaRequired: e.code === "MFA_REQUIRED",
			methods: Array.isArray(methods)
				? methods.filter((m): m is string => typeof m === "string")
				: undefined,
			errorCode: e.code,
			message: e.message || "Could not change your password.",
		};
	}
}

export function sendChangePasswordEmailCode(): Promise<{ message: string }> {
	return authFetch<{ message: string }>("/change-password/send-email-code", {
		method: "POST",
	});
}

export interface LoginHistoryItem {
	attemptType: string;
	outcome: string;
	failureReason?: string;
	ipAddress?: string;
	userAgent?: string;
	attemptedAt: string;
}

/** `GET /auth/login-history` — the caller's 20 most recent sign-ins. */
export function getLoginHistory(): Promise<{ attempts: LoginHistoryItem[] }> {
	return authFetch<{ attempts: LoginHistoryItem[] }>("/login-history");
}
