/**
 * Sync result types returned by `DefinitionSynchronizer`.
 *
 * Each per-category result mirrors the backend's `SyncResultResponse` shape
 * (what the platform wrote), plus a convenience `skipped` flag set when the
 * category wasn't present in the input `DefinitionSet`.
 */

/** Per-category counts. */
export interface CategorySyncResult {
	applicationCode: string;
	created: number;
	updated: number;
	deleted: number;
	syncedCodes: string[];
	/**
	 * Set when this category could not be fully synced without aborting
	 * sibling categories — a local configuration error (e.g. a duplicate
	 * code within one scope) or a scope whose subscriptions were skipped
	 * because that scope's connection sync failed first. Counts reflect
	 * only what DID succeed; `syncedCodes` likewise.
	 */
	error?: string;
}

/** Sentinel returned when a category wasn't part of the submitted set. */
export interface SkippedSyncResult {
	skipped: true;
}

export type MaybeCategoryResult = CategorySyncResult | SkippedSyncResult;

/** Aggregate result of syncing a full `DefinitionSet`. */
export interface SyncResult {
	applicationCode: string;
	roles: MaybeCategoryResult;
	eventTypes: MaybeCategoryResult;
	/** Always synced before `subscriptions` — see `DefinitionSet.connections`. */
	connections: MaybeCategoryResult;
	subscriptions: MaybeCategoryResult;
	dispatchPools: MaybeCategoryResult;
	principals: MaybeCategoryResult;
	processes: MaybeCategoryResult;
	scheduledJobs: MaybeCategoryResult;
	/**
	 * OpenAPI sync is a single-document upload. On success
	 * `syncedCodes` carries `[version]`; `created`/`updated` reflect
	 * whether the document was newly published or replaced an existing
	 * one (both zero if byte-identical re-sync).
	 */
	openapi: MaybeCategoryResult;
}

/** Narrow a category result to the non-skipped case. */
export function isSynced(r: MaybeCategoryResult): r is CategorySyncResult {
	return !("skipped" in r && r.skipped);
}

/** A "skipped" sentinel for categories not in the set. */
export const SKIPPED: SkippedSyncResult = { skipped: true };
