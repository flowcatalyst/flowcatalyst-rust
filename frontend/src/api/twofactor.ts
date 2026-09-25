// Two-factor authentication (Go's contract). Endpoints live under /auth/2fa/*
// (NOT /api), like the rest of the auth surface; errors are shown inline by
// the calling form, so the transport raises no global toast.
import {
	applyLoginSuccess,
	setSessionUser,
	type RawLoginResponse,
} from "./auth";
import { authFetch } from "./auth-fetch";

export { redirectAfterLogin } from "./auth";

export type TwoFactorMethod = "TOTP" | "EMAIL_PIN";
export type ChallengeMethod = TwoFactorMethod | "RECOVERY_CODE";

const post = <T>(path: string, body?: unknown) =>
	authFetch<T>(path, { method: "POST", body: JSON.stringify(body ?? {}) });
const get = <T>(path: string) => authFetch<T>(path);
const del = <T>(path: string) => authFetch<T>(path, { method: "DELETE" });

export interface TotpEnrollment {
	secret: string;
	/** The `otpauth://` URI. */
	uri: string;
	/** PNG data URI of the otpauth QR code (empty if rendering failed). */
	qr?: string;
}

// ── login challenge (mfa-token-gated) ──────────────────────────────────────

/** Verify a second factor; on success the session is set and we navigate. */
export async function verifyTwoFactor(args: {
	mfaToken: string;
	method: ChallengeMethod;
	code: string;
	rememberDevice?: boolean;
}): Promise<void> {
	const data = await post<RawLoginResponse>("/2fa/verify", args);
	await applyLoginSuccess(data);
}

export function sendEmailChallenge(
	mfaToken: string,
): Promise<{ message: string }> {
	return post("/2fa/challenge/email", { mfaToken });
}

// ── enrolment during login/reset (enroll-token-gated) ──────────────────────
// The confirm endpoints set the session cookie and complete the sign-in.
// Navigation is deferred so the caller can show the recovery codes first,
// then call redirectAfterLogin().

export function enrollTotpBegin(enrollToken: string): Promise<TotpEnrollment> {
	return post("/2fa/enroll/totp/begin", { enrollToken });
}

export async function enrollTotpConfirm(
	enrollToken: string,
	code: string,
): Promise<string[]> {
	const data = await post<RawLoginResponse>("/2fa/enroll/totp/confirm", {
		enrollToken,
		code,
	});
	await setSessionUser(data);
	return data.recoveryCodes ?? [];
}

export function enrollEmailBegin(
	enrollToken: string,
): Promise<{ message: string }> {
	return post("/2fa/enroll/email/begin", { enrollToken });
}

export async function enrollEmailConfirm(
	enrollToken: string,
	code: string,
): Promise<string[]> {
	const data = await post<RawLoginResponse>("/2fa/enroll/email/confirm", {
		enrollToken,
		code,
	});
	await setSessionUser(data);
	return data.recoveryCodes ?? [];
}

// ── self-service (session-gated) ───────────────────────────────────────────

export interface TwoFactorStatus {
	/** Confirmed methods. */
	methods: TwoFactorMethod[];
	/** The user's email domain requires 2FA. */
	required: boolean;
	allowedMethods: TwoFactorMethod[];
	recoveryCodesLeft: number;
	rememberDeviceEnabled: boolean;
	trustedDeviceCount: number;
}

export function getTwoFactorStatus(): Promise<TwoFactorStatus> {
	return get("/2fa/status");
}

export function selfEnrollTotpBegin(): Promise<TotpEnrollment> {
	return post("/2fa/methods/totp/begin");
}

export function selfEnrollTotpConfirm(
	code: string,
): Promise<{ recoveryCodes: string[] }> {
	return post("/2fa/methods/totp/confirm", { code });
}

export function selfEnrollEmailBegin(): Promise<{ message: string }> {
	return post("/2fa/methods/email/begin");
}

export function selfEnrollEmailConfirm(
	code: string,
): Promise<{ recoveryCodes: string[] }> {
	return post("/2fa/methods/email/confirm", { code });
}

export function removeTwoFactorMethod(
	method: TwoFactorMethod,
): Promise<{ message: string }> {
	return del(`/2fa/methods/${method}`);
}

export function regenerateRecoveryCodes(): Promise<{ recoveryCodes: string[] }> {
	return post("/2fa/recovery-codes/regenerate");
}

export interface TrustedDevice {
	id: string;
	label?: string;
	createdAt: string;
	lastUsedAt?: string;
	expiresAt: string;
}

export function listTrustedDevices(): Promise<{ devices: TrustedDevice[] }> {
	return get("/2fa/trusted-devices");
}

export function revokeTrustedDevice(id: string): Promise<{ message: string }> {
	return del(`/2fa/trusted-devices/${encodeURIComponent(id)}`);
}

export function methodLabel(method: string): string {
	switch (method) {
		case "TOTP":
			return "Authenticator app";
		case "EMAIL_PIN":
			return "Email code";
		default:
			return method;
	}
}
