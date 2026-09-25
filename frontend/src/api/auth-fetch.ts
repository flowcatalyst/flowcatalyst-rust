/**
 * Transport for the auth surface (`/auth/*`, not `/api`): login, two-factor,
 * password flows and account self-service.
 *
 * Unlike `apiFetch`, it raises no global error toast and no 401/403 event —
 * every caller is a form that shows its error inline, and a 401 here means
 * "wrong code" or "token expired", not "your session ended". Errors are
 * thrown as `ApiError` carrying the human message (the platform's
 * `{ code, message }` / `{ error, message }` envelope) and the code.
 */

import { ApiError } from "./client";

export const AUTH_BASE_URL = "/auth";

export async function authFetch<T>(
	path: string,
	init: RequestInit = {},
): Promise<T> {
	const headers: Record<string, string> = {
		...(init.headers as Record<string, string>),
	};
	if (init.body && !headers["Content-Type"]) {
		headers["Content-Type"] = "application/json";
	}

	const response = await fetch(`${AUTH_BASE_URL}${path}`, {
		...init,
		credentials: "include",
		headers,
	});

	if (!response.ok) {
		const body = (await response.json().catch(() => ({}))) as Record<
			string,
			unknown
		>;
		const code =
			(typeof body["code"] === "string" && body["code"]) ||
			(typeof body["error"] === "string" && body["error"]) ||
			undefined;
		const message =
			(typeof body["message"] === "string" && body["message"]) ||
			code ||
			"Request failed";
		throw new ApiError(message, response.status, code, body);
	}

	if (response.status === 204) {
		return undefined as T;
	}
	const text = await response.text();
	return (text ? JSON.parse(text) : undefined) as T;
}
