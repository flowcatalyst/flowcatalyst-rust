/**
 * Sealed Result<T> for use case outcomes.
 *
 * Success values can only be constructed through a `UnitOfWork` — the
 * `success()` factory requires a token that is `internal` to this module
 * and exported only to the unit-of-work implementation. This guarantees
 * domain events always get dispatched when state changes.
 */

import type { UseCaseError } from "./errors.js";

/** @internal */
export const RESULT_SUCCESS_TOKEN: unique symbol = Symbol(
	"RESULT_SUCCESS_TOKEN",
);
/** @internal */
export type ResultSuccessToken = typeof RESULT_SUCCESS_TOKEN;

export interface Success<T> {
	readonly _tag: "success";
	readonly value: T;
}

export interface Failure<_T> {
	readonly _tag: "failure";
	readonly error: UseCaseError;
}

export type Result<T> = Success<T> | Failure<T>;

export function isSuccess<T>(r: Result<T>): r is Success<T> {
	return r._tag === "success";
}
export function isFailure<T>(r: Result<T>): r is Failure<T> {
	return r._tag === "failure";
}

export const Result = {
	/**
	 * Create a success. Restricted — only UnitOfWork implementations hold the
	 * token. Use cases must route success through `unitOfWork.commit(...)`.
	 * @internal
	 */
	success<T>(token: ResultSuccessToken, value: T): Success<T> {
		if (token !== RESULT_SUCCESS_TOKEN) {
			throw new Error(
				"Result.success() is restricted. Return success via UnitOfWork.commit() so domain events are always dispatched.",
			);
		}
		return { _tag: "success", value };
	},

	failure<T>(error: UseCaseError): Failure<T> {
		return { _tag: "failure", error };
	},

	isSuccess,
	isFailure,

	map<T, U>(r: Result<T>, fn: (v: T) => U): Result<U> {
		return isSuccess(r)
			? { _tag: "success", value: fn(r.value) }
			: { _tag: "failure", error: r.error };
	},

	match<T, U>(
		r: Result<T>,
		onSuccess: (v: T) => U,
		onFailure: (e: UseCaseError) => U,
	): U {
		return isSuccess(r) ? onSuccess(r.value) : onFailure(r.error);
	},

	unwrap<T>(r: Result<T>): T {
		if (isSuccess(r)) return r.value;
		throw new Error(`Result is failure: ${r.error.code} — ${r.error.message}`);
	},
};
