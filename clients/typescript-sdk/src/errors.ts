/**
 * SDK Error Types
 *
 * Discriminated union types for typed error handling with neverthrow.
 * All errors have a `type` field for pattern matching.
 */

/** Authentication errors */
export type AuthenticationError =
	| { type: "missing_credentials"; message: string }
	| { type: "invalid_credentials"; message: string }
	| { type: "token_expired"; message: string }
	| { type: "token_fetch_failed"; message: string; cause?: Error };

/** HTTP/Network errors */
export type HttpError =
	| { type: "network"; message: string; cause?: Error }
	| { type: "timeout"; message: string; durationMs: number }
	| { type: "http_error"; status: number; message: string; body?: unknown };

/** Validation errors (422) */
export type ValidationError = {
	type: "validation";
	message: string;
	errors: Record<string, string[]>;
};

/** Not found errors (404) */
export type NotFoundError = {
	type: "not_found";
	message: string;
	resource?: string;
	id?: string;
};

/** Forbidden errors (403) */
export type ForbiddenError = {
	type: "forbidden";
	message: string;
};

/** Conflict errors (409) */
export type ConflictError = {
	type: "conflict";
	message: string;
	code?: string;
};

/** Rate limit errors (429) */
export type RateLimitError = {
	type: "rate_limited";
	message: string;
	retryAfterMs?: number;
};

/**
 * A multi-step operation (currently: `DefinitionSynchronizer.sync()` /
 * `syncGrouped()` / `syncAll()`) completed the whole call but one or more of
 * its steps failed. Generic over the shape of "whatever succeeded" — a
 * caller must not be forced to discard good work just because part of a
 * batch failed, so the partial result rides on the error itself rather than
 * being lost when the call fails loud.
 *
 * Deliberately NOT part of the base `SdkError` union: `partial` is only
 * meaningfully typed per call site (e.g. `SyncResult` for `sync()`,
 * `SyncResult[]` for `syncAll()`), and folding it into `SdkError` would
 * either lose that typing (widen `partial` to `unknown` everywhere) or force
 * every unrelated `SdkError` consumer to account for a variant it can't
 * produce. Operations that can return this instead widen their own return
 * type, e.g. `ResultAsync<SyncResult, SdkError | PartialFailureError<SyncResult>>`.
 */
export type PartialFailureError<T> = {
	type: "partial_failure";
	message: string;
	partial: T;
};

/** Union of all SDK errors */
export type SdkError =
	| AuthenticationError
	| HttpError
	| ValidationError
	| NotFoundError
	| ForbiddenError
	| ConflictError
	| RateLimitError;

/**
 * Create an authentication error
 */
export const authError = {
	missingCredentials: (
		message = "Client ID and secret are required",
	): AuthenticationError => ({
		type: "missing_credentials",
		message,
	}),
	invalidCredentials: (
		message = "Invalid client credentials",
	): AuthenticationError => ({
		type: "invalid_credentials",
		message,
	}),
	tokenExpired: (
		message = "Access token has expired",
	): AuthenticationError => ({
		type: "token_expired",
		message,
	}),
	tokenFetchFailed: (message: string, cause?: Error): AuthenticationError => ({
		type: "token_fetch_failed",
		message,
		cause,
	}),
};

/**
 * Create an HTTP error
 */
export const httpError = {
	network: (message: string, cause?: Error): HttpError => ({
		type: "network",
		message,
		cause,
	}),
	timeout: (durationMs: number): HttpError => ({
		type: "timeout",
		message: `Request timed out after ${durationMs}ms`,
		durationMs,
	}),
	http: (status: number, message: string, body?: unknown): HttpError => ({
		type: "http_error",
		status,
		message,
		body,
	}),
};

/**
 * Create a validation error from API response
 */
export const validationError = (
	message: string,
	errors: Record<string, string[]> = {},
): ValidationError => ({
	type: "validation",
	message,
	errors,
});

/**
 * Create a not found error
 */
export const notFoundError = (
	message: string,
	resource?: string,
	id?: string,
): NotFoundError => ({
	type: "not_found",
	message,
	resource,
	id,
});

/**
 * Create a forbidden error
 */
export const forbiddenError = (
	message = "Access forbidden",
): ForbiddenError => ({
	type: "forbidden",
	message,
});

/**
 * Create a conflict error
 */
export const conflictError = (
	message: string,
	code?: string,
): ConflictError => ({
	type: "conflict",
	message,
	code,
});

/**
 * Create a rate limit error
 */
export const rateLimitError = (
	message: string,
	retryAfterMs?: number,
): RateLimitError => ({
	type: "rate_limited",
	message,
	retryAfterMs,
});

/**
 * Create a partial-failure error, carrying whatever succeeded so it isn't
 * lost when the overall call reports failure. See {@link PartialFailureError}.
 */
export function partialFailureError<T>(
	message: string,
	partial: T,
): PartialFailureError<T> {
	return { type: "partial_failure", message, partial };
}

/** Narrow an error to a {@link PartialFailureError}, recovering its `partial` payload's type. */
export function isPartialFailureError<T>(
	error: unknown,
): error is PartialFailureError<T> {
	return (
		typeof error === "object" &&
		error !== null &&
		(error as { type?: unknown }).type === "partial_failure"
	);
}

/**
 * Map HTTP status code to appropriate error
 */
export function mapHttpStatusToError(
	status: number,
	body?: unknown,
	message?: string,
): SdkError {
	const errorBody = body as Record<string, unknown> | undefined;
	// Platform JSON: { error: "<CODE>", message: "<human text>" }
	// Prefer the human message; fall back to the code, then a generic.
	const errorMessage =
		message ??
		errorBody?.["message"]?.toString() ??
		errorBody?.["error"]?.toString() ??
		`HTTP ${status}`;
	const errorCode = errorBody?.["error"]?.toString();

	switch (status) {
		case 401:
			return authError.tokenExpired(errorMessage);
		case 403:
			return forbiddenError(errorMessage);
		case 404:
			return notFoundError(errorMessage);
		case 409:
			return conflictError(errorMessage, errorCode);
		case 422:
			return validationError(
				errorMessage,
				(errorBody?.["errors"] as Record<string, string[]>) ?? {},
			);
		case 429: {
			const retryAfter = errorBody?.["retryAfter"] as number | undefined;
			return rateLimitError(
				errorMessage,
				retryAfter ? retryAfter * 1000 : undefined,
			);
		}
		default:
			return httpError.http(status, errorMessage, body);
	}
}
