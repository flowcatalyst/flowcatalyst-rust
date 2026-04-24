/**
 * DefinitionSynchronizer — orchestrates syncing a `DefinitionSet` to the
 * platform's application-scoped sync API (`/api/applications/{app}/*\/sync`).
 *
 * One orchestrator per `FlowCatalystClient`; auth/retry/errors are delegated
 * to the client's shared request pipeline.
 */
import { okAsync } from "neverthrow";
import { SKIPPED } from "./result";
/**
 * Sync FlowCatalyst definitions to the platform.
 *
 * Construct via `client.definitions()`; the orchestrator reuses the
 * client's auth, retry, and error handling.
 */
export class DefinitionSynchronizer {
    constructor(client) {
        this.client = client;
    }
    /**
     * Sync one application's definitions.
     *
     * Categories are sync'd in a fixed order — roles, event types,
     * subscriptions, dispatch pools, principals — so that subscriptions
     * can reference the event types and dispatch pools that were just
     * created. Each category sync is an independent HTTP call; a failure
     * in one does NOT roll back earlier successes.
     */
    sync(set, options = {}) {
        const removeUnlisted = options.removeUnlisted ?? false;
        const rolesStep = () => options.skipRoles || !set.roles
            ? okAsync(SKIPPED)
            : this.syncRoles(set.applicationCode, set.roles, removeUnlisted);
        const eventTypesStep = () => options.skipEventTypes || !set.eventTypes
            ? okAsync(SKIPPED)
            : this.syncEventTypes(set.applicationCode, set.eventTypes, removeUnlisted);
        const subsStep = () => options.skipSubscriptions || !set.subscriptions
            ? okAsync(SKIPPED)
            : this.syncSubscriptions(set.applicationCode, set.subscriptions, removeUnlisted);
        const poolsStep = () => options.skipDispatchPools || !set.dispatchPools
            ? okAsync(SKIPPED)
            : this.syncDispatchPools(set.applicationCode, set.dispatchPools, removeUnlisted);
        const principalsStep = () => options.skipPrincipals || !set.principals
            ? okAsync(SKIPPED)
            : this.syncPrincipals(set.applicationCode, set.principals, removeUnlisted);
        return rolesStep()
            .andThen((roles) => eventTypesStep().map((eventTypes) => ({ roles, eventTypes })))
            .andThen((acc) => subsStep().map((subscriptions) => ({ ...acc, subscriptions })))
            .andThen((acc) => poolsStep().map((dispatchPools) => ({ ...acc, dispatchPools })))
            .andThen((acc) => principalsStep().map((principals) => ({
            applicationCode: set.applicationCode,
            ...acc,
            principals,
        })));
    }
    /**
     * Sync multiple applications' definitions. Each set is sync'd
     * sequentially; results are returned in the same order as `sets`.
     * A failure in one set short-circuits the rest.
     */
    syncAll(sets, options = {}) {
        return sets.reduce((chain, set) => chain.andThen((acc) => this.sync(set, options).map((result) => [...acc, result])), okAsync([]));
    }
    // ── per-category callers ──────────────────────────────────────────
    syncRoles(applicationCode, roles, removeUnlisted) {
        return this.post(applicationCode, "roles", { roles }, removeUnlisted);
    }
    syncEventTypes(applicationCode, eventTypes, removeUnlisted) {
        return this.post(applicationCode, "event-types", { eventTypes }, removeUnlisted);
    }
    syncSubscriptions(applicationCode, subscriptions, removeUnlisted) {
        return this.post(applicationCode, "subscriptions", { subscriptions }, removeUnlisted);
    }
    syncDispatchPools(applicationCode, pools, removeUnlisted) {
        return this.post(applicationCode, "dispatch-pools", { pools }, removeUnlisted);
    }
    syncPrincipals(applicationCode, principals, removeUnlisted) {
        return this.post(applicationCode, "principals", { principals }, removeUnlisted);
    }
    // ── transport ─────────────────────────────────────────────────────
    post(applicationCode, resource, body, removeUnlisted) {
        return this.client.request((httpClient, headers) => httpClient.post({
            url: `/api/applications/${applicationCode}/${resource}/sync`,
            headers: {
                ...headers,
                "Content-Type": "application/json",
            },
            body,
            query: { removeUnlisted },
        }));
    }
}
