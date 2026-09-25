// Stays hand-rolled: auth surface. The routes ARE in the OpenAPI spec
// (huma), but the browser-ceremony payloads (navigator.credentials shapes,
// Set-Cookie session handling) don't map onto the generated types usefully.
/**
 * WebAuthn / passkey API client.
 *
 * Wraps the six endpoints under `/auth/webauthn/*`. Browser ceremonies
 * (`navigator.credentials.create()` / `.get()`) are driven by
 * `@simplewebauthn/browser`, which handles the ArrayBuffer ↔ base64url
 * conversions for us.
 *
 * The federated-domain gate, the username-enumeration defence, and the
 * exponential backoff all live on the server — the client just calls the
 * endpoints and surfaces any 4xx the server returns. Specifically: the
 * authenticate-begin response is identical for known and unknown emails,
 * so failures only show up at /complete time as a generic 401.
 */

import {
	startRegistration,
	startAuthentication,
} from "@simplewebauthn/browser";
import type {
	PublicKeyCredentialCreationOptionsJSON,
	PublicKeyCredentialRequestOptionsJSON,
	RegistrationResponseJSON,
	AuthenticationResponseJSON,
} from "@simplewebauthn/browser";

import { authFetch } from "./client";

// Paths below are relative to authFetch's /auth prefix.
const WEBAUTHN = "/webauthn";

export interface CredentialSummary {
	id: string;
	name: string | null;
	createdAt: string;
	lastUsedAt: string | null;
}

interface RegisterBeginResponse {
	stateId: string;
	options: { publicKey: PublicKeyCredentialCreationOptionsJSON };
}

interface RegisterCompleteResponse {
	credentialId: string;
}

interface AuthenticateBeginResponse {
	stateId: string;
	options: { publicKey: PublicKeyCredentialRequestOptionsJSON };
}

interface AuthenticateCompleteResponse {
	principalId: string;
	email: string | null;
	name: string;
	roles: string[];
}

const postJson = <T>(path: string, body: unknown) =>
	authFetch<T>(`${WEBAUTHN}${path}`, {
		method: "POST",
		body: JSON.stringify(body),
	});

/**
 * Register a new passkey for the currently authenticated user.
 *
 * Throws if the user's domain is federated, the browser cancels, or the
 * server rejects the attestation.
 */
export async function registerPasskey(name?: string): Promise<string> {
	const begin = await postJson<RegisterBeginResponse>("/register/begin", {
		displayName: name,
	});

	// startRegistration prompts the authenticator (Touch ID, YubiKey, etc.)
	// and returns the attestation response in the JSON shape the server
	// expects.
	const credential: RegistrationResponseJSON = await startRegistration({
		optionsJSON: begin.options.publicKey,
	});

	const result = await postJson<RegisterCompleteResponse>("/register/complete", {
		stateId: begin.stateId,
		name: name ?? null,
		credential,
	});
	return result.credentialId;
}

/**
 * Authenticate with a registered passkey.
 *
 * Returns the principal info on success. On failure (cancelled, no matching
 * credential, federated domain, rate-limited, etc.) the server responds 401
 * with a generic message — the caller should not distinguish reasons.
 */
export async function authenticateWithPasskey(
	email: string,
): Promise<AuthenticateCompleteResponse> {
	const begin = await postJson<AuthenticateBeginResponse>(
		"/authenticate/begin",
		{ email },
	);

	const credential: AuthenticationResponseJSON = await startAuthentication({
		optionsJSON: begin.options.publicKey,
	});

	return postJson<AuthenticateCompleteResponse>("/authenticate/complete", {
		stateId: begin.stateId,
		credential,
	});
}

/** List the current user's registered passkeys. */
export async function listPasskeys(): Promise<CredentialSummary[]> {
	return authFetch<CredentialSummary[]>(`${WEBAUTHN}/credentials`);
}

/** Revoke a specific passkey. */
export async function revokePasskey(credentialId: string): Promise<void> {
	await authFetch<void>(
		`${WEBAUTHN}/credentials/${encodeURIComponent(credentialId)}`,
		{ method: "DELETE" },
	);
}

/**
 * Whether the current browser supports WebAuthn at all. Returns false on
 * very old browsers and on insecure (non-https / non-localhost) origins.
 */
export function isWebauthnSupported(): boolean {
	return (
		typeof window !== "undefined" &&
		typeof window.PublicKeyCredential !== "undefined" &&
		typeof navigator.credentials?.create === "function"
	);
}

/**
 * Whether platform authenticators (Touch ID, Face ID, Windows Hello) are
 * available. Used to gate UI hints — never to gate the actual ceremony,
 * which can also use roaming authenticators (YubiKey, phone-as-passkey).
 */
export async function isPlatformAuthenticatorAvailable(): Promise<boolean> {
	if (!isWebauthnSupported()) return false;
	try {
		return await window.PublicKeyCredential.isUserVerifyingPlatformAuthenticatorAvailable();
	} catch {
		return false;
	}
}
