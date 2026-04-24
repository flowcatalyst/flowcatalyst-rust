/**
 * Sync result types returned by `DefinitionSynchronizer`.
 *
 * Each per-category result mirrors the backend's `SyncResultResponse` shape
 * (what the platform wrote), plus a convenience `skipped` flag set when the
 * category wasn't present in the input `DefinitionSet`.
 */
/** Narrow a category result to the non-skipped case. */
export function isSynced(r) {
    return !("skipped" in r && r.skipped);
}
/** A "skipped" sentinel for categories not in the set. */
export const SKIPPED = { skipped: true };
