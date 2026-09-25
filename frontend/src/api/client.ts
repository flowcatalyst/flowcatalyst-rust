/**
 * The app's HTTP transport: thin fetch wrappers with the platform error
 * envelope decoded once — global toasts, 401/403 events, and per-field
 * validation detail extraction live here so feature modules don't
 * reimplement them.
 */

import { toast } from "@/utils/errorBus";

export const API_BASE_URL = "/api";
export const BFF_BASE_URL = "/bff";
export const AUTH_BASE_URL = "/auth";

/**
 * Custom error class for API errors that includes status code
 */
export class ApiError extends Error {
	status: number;
	code?: string;

	constructor(
		message: string,
		status: number,
		code?: string,
	) {
		super(message);
		this.name = "ApiError";
		this.status = status;
		this.code = code;
	}
}

/**
 * Event emitter for API errors (401/403)
 */
type ApiErrorListener = (status: number, message: string) => void;
const errorListeners: ApiErrorListener[] = [];

export function onApiError(listener: ApiErrorListener): () => void {
	errorListeners.push(listener);
	return () => {
		const index = errorListeners.indexOf(listener);
		if (index > -1) {
			errorListeners.splice(index, 1);
		}
	};
}

function emitApiError(status: number, message: string) {
	errorListeners.forEach((listener) => listener(status, message));
}

function summaryForStatus(status: number): string {
	if (status === 400) return "Invalid Request";
	if (status === 403) return "Access Denied";
	if (status === 404) return "Not Found";
	if (status === 409) return "Conflict";
	if (status === 422) return "Validation Failed";
	if (status >= 500) return "Server Error";
	return "Request Failed";
}

/**
 * Extra options accepted by `apiFetch` / `bffFetch` on top of the standard
 * `RequestInit` fields.
 */
export interface FetchOptions extends RequestInit {
	/**
	 * If true, suppress the global error banner on a non-2xx response. The
	 * caller is then responsible for surfacing the error to the user (e.g. an
	 * inline form message). The error is still thrown as an `ApiError`.
	 */
	suppressGlobalErrorToast?: boolean;
	/**
	 * If true, suppress the global 401/403 event (the "session expired" /
	 * permission-denied modal). The auth surface sets this: a wrong password
	 * or expired reset token is a form-level error, not an expired session.
	 */
	suppressAuthErrorEvent?: boolean;
}

/**
 * Fetch from the main API endpoints.
 */
export async function apiFetch<T>(
	path: string,
	options: FetchOptions = {},
): Promise<T> {
	return baseFetch<T>(`${API_BASE_URL}${path}`, options);
}

/**
 * Fetch from BFF (Backend For Frontend) endpoints.
 * BFF endpoints return IDs as strings to preserve precision for JavaScript.
 */
export async function bffFetch<T>(
	path: string,
	options: FetchOptions = {},
): Promise<T> {
	return baseFetch<T>(`${BFF_BASE_URL}${path}`, options);
}

/**
 * Fetch from the /auth/* surface (login, 2FA, passkeys, password reset).
 * Same envelope decoding as apiFetch, but errors stay inline by default:
 * no global toast, and no 401/403 modal — a failed credential is the
 * form's problem, not a session expiry.
 */
export async function authFetch<T>(
	path: string,
	options: FetchOptions = {},
): Promise<T> {
	return baseFetch<T>(`${AUTH_BASE_URL}${path}`, {
		suppressGlobalErrorToast: true,
		suppressAuthErrorEvent: true,
		...options,
	});
}

async function baseFetch<T>(
	url: string,
	options: FetchOptions = {},
): Promise<T> {
	const { suppressGlobalErrorToast, suppressAuthErrorEvent, ...init } =
		options;

	const headers: Record<string, string> = {
		...(init.headers as Record<string, string>),
	};
	if (init.body) {
		headers["Content-Type"] = "application/json";
	}

	const response = await fetch(url, {
		...init,
		credentials: "include",
		headers,
	});

	if (!response.ok) {
		const error = await response
			.json()
			.catch(() => ({ message: "Request failed" }));
		// Platform JSON: { error: "<CODE>", message: "<human text>",
		//   details?: { errors?: [{ message, location, value }] } }
		// Prefer the human message; fall back to the code, then a generic.
		let message =
			(typeof error.message === "string" && error.message) ||
			(typeof error.error === "string" && error.error) ||
			"Request failed";
		// Surface per-field validation details so the caller sees WHICH field
		// failed (e.g. "body.redirectUris: ...") rather than a bare
		// "validation failed".
		const fieldErrors = (error?.details?.errors ?? []) as Array<{
			message?: string;
			location?: string;
		}>;
		if (Array.isArray(fieldErrors) && fieldErrors.length > 0) {
			const parts = fieldErrors
				.map((fe) =>
					fe.location ? `${fe.location}: ${fe.message ?? ""}`.trim() : fe.message,
				)
				.filter((p): p is string => !!p);
			if (parts.length > 0) message = `${message} (${parts.join("; ")})`;
		}
		const code =
			(typeof error.error === "string" && error.error) || error.code;

		// Emit error event for 401/403 (session-expired / permission modal)
		// unless the caller opted out (auth surface).
		if (
			(response.status === 401 || response.status === 403) &&
			!suppressAuthErrorEvent
		) {
			emitApiError(response.status, message);
		}

		// Show error banner for non-auth errors unless the caller opted out.
		if (response.status !== 401 && !suppressGlobalErrorToast) {
			toast.error(summaryForStatus(response.status), message);
		}

		throw new ApiError(message, response.status, code);
	}

	// Handle 204 No Content
	if (response.status === 204) {
		return undefined as T;
	}

	// Tolerate empty 200 bodies (some auth endpoints reply 200 with no
	// payload) — response.json() would throw on them.
	const text = await response.text();
	return (text ? JSON.parse(text) : undefined) as T;
}
