/**
 * Sealed Result<T> for use case outcomes.
 *
 * Success values can only be constructed through a `UnitOfWork` — the
 * `success()` factory requires a token that is `internal` to this module
 * and exported only to the unit-of-work implementation. This guarantees
 * domain events always get dispatched when state changes.
 */
/** @internal */
export const RESULT_SUCCESS_TOKEN = Symbol("RESULT_SUCCESS_TOKEN");
export function isSuccess(r) {
    return r._tag === "success";
}
export function isFailure(r) {
    return r._tag === "failure";
}
export const Result = {
    /**
     * Create a success. Restricted — only UnitOfWork implementations hold the
     * token. Use cases must route success through `unitOfWork.commit(...)`.
     * @internal
     */
    success(token, value) {
        if (token !== RESULT_SUCCESS_TOKEN) {
            throw new Error("Result.success() is restricted. Return success via UnitOfWork.commit() so domain events are always dispatched.");
        }
        return { _tag: "success", value };
    },
    failure(error) {
        return { _tag: "failure", error };
    },
    isSuccess,
    isFailure,
    map(r, fn) {
        return isSuccess(r)
            ? { _tag: "success", value: fn(r.value) }
            : { _tag: "failure", error: r.error };
    },
    match(r, onSuccess, onFailure) {
        return isSuccess(r) ? onSuccess(r.value) : onFailure(r.error);
    },
    unwrap(r) {
        if (isSuccess(r))
            return r.value;
        throw new Error(`Result is failure: ${r.error.code} — ${r.error.message}`);
    },
};
