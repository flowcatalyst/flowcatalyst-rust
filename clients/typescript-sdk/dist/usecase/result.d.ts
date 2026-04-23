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
export declare const RESULT_SUCCESS_TOKEN: unique symbol;
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
export declare function isSuccess<T>(r: Result<T>): r is Success<T>;
export declare function isFailure<T>(r: Result<T>): r is Failure<T>;
export declare const Result: {
    /**
     * Create a success. Restricted — only UnitOfWork implementations hold the
     * token. Use cases must route success through `unitOfWork.commit(...)`.
     * @internal
     */
    success<T>(token: ResultSuccessToken, value: T): Success<T>;
    failure<T>(error: UseCaseError): Failure<T>;
    isSuccess: typeof isSuccess;
    isFailure: typeof isFailure;
    map<T, U>(r: Result<T>, fn: (v: T) => U): Result<U>;
    match<T, U>(r: Result<T>, onSuccess: (v: T) => U, onFailure: (e: UseCaseError) => U): U;
    unwrap<T>(r: Result<T>): T;
};
//# sourceMappingURL=result.d.ts.map