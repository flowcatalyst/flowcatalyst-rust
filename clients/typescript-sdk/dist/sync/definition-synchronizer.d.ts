/**
 * DefinitionSynchronizer — orchestrates syncing a `DefinitionSet` to the
 * platform's application-scoped sync API (`/api/applications/{app}/*\/sync`).
 *
 * One orchestrator per `FlowCatalystClient`; auth/retry/errors are delegated
 * to the client's shared request pipeline.
 */
import { type ResultAsync } from "neverthrow";
import type { FlowCatalystClient } from "../client";
import type { SdkError } from "../errors";
import type { DefinitionSet } from "./definitions";
import type { SyncResult } from "./result";
/** Options for a sync call. */
export interface SyncOptions {
    /**
     * When true, the platform removes SDK-sourced rows not present in the
     * submitted list (per category). Rows created through the admin UI are
     * preserved regardless. Default: false.
     */
    removeUnlisted?: boolean;
    /**
     * Per-category opt-out. Omitting a category from the `DefinitionSet`
     * already skips it; these flags let you force-skip categories even if
     * they're present (e.g. to stage a rollout).
     */
    skipRoles?: boolean;
    skipEventTypes?: boolean;
    skipSubscriptions?: boolean;
    skipDispatchPools?: boolean;
    skipPrincipals?: boolean;
}
/**
 * Sync FlowCatalyst definitions to the platform.
 *
 * Construct via `client.definitions()`; the orchestrator reuses the
 * client's auth, retry, and error handling.
 */
export declare class DefinitionSynchronizer {
    private readonly client;
    constructor(client: FlowCatalystClient);
    /**
     * Sync one application's definitions.
     *
     * Categories are sync'd in a fixed order — roles, event types,
     * subscriptions, dispatch pools, principals — so that subscriptions
     * can reference the event types and dispatch pools that were just
     * created. Each category sync is an independent HTTP call; a failure
     * in one does NOT roll back earlier successes.
     */
    sync(set: DefinitionSet, options?: SyncOptions): ResultAsync<SyncResult, SdkError>;
    /**
     * Sync multiple applications' definitions. Each set is sync'd
     * sequentially; results are returned in the same order as `sets`.
     * A failure in one set short-circuits the rest.
     */
    syncAll(sets: DefinitionSet[], options?: SyncOptions): ResultAsync<SyncResult[], SdkError>;
    private syncRoles;
    private syncEventTypes;
    private syncSubscriptions;
    private syncDispatchPools;
    private syncPrincipals;
    private post;
}
//# sourceMappingURL=definition-synchronizer.d.ts.map