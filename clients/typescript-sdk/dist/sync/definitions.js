/**
 * Definition types for syncing FlowCatalyst primitives to the platform.
 *
 * These types describe the shape of things an application declares about
 * itself: the roles it needs, the event types it publishes, the
 * subscriptions it consumes, the dispatch pools it expects, and the
 * principals it manages.
 *
 * You build a `DefinitionSet` (one per application) and pass it to
 * `client.definitions().sync(...)`. See `docs/syncing-definitions.md` for
 * a full walkthrough.
 */
/**
 * Fluent builder for `DefinitionSet`. Mirrors the Laravel SDK's
 * `SyncDefinitionSet` shape so definitions can be described the same way
 * across languages.
 *
 * Example:
 * ```ts
 * const set = defineApplication("orders")
 *   .withRoles([{ name: "admin", displayName: "Administrator" }])
 *   .withEventTypes([
 *     { code: "orders:fulfillment:shipment:shipped", name: "Shipment Shipped" },
 *   ])
 *   .build();
 * ```
 */
export class DefinitionSetBuilder {
    constructor(applicationCode) {
        this.set = { applicationCode };
    }
    withRoles(roles) {
        this.set.roles = [...(this.set.roles ?? []), ...roles];
        return this;
    }
    withEventTypes(eventTypes) {
        this.set.eventTypes = [...(this.set.eventTypes ?? []), ...eventTypes];
        return this;
    }
    withSubscriptions(subscriptions) {
        this.set.subscriptions = [
            ...(this.set.subscriptions ?? []),
            ...subscriptions,
        ];
        return this;
    }
    withDispatchPools(pools) {
        this.set.dispatchPools = [...(this.set.dispatchPools ?? []), ...pools];
        return this;
    }
    withPrincipals(principals) {
        this.set.principals = [...(this.set.principals ?? []), ...principals];
        return this;
    }
    build() {
        return { ...this.set };
    }
}
/** Convenience: start building definitions for `applicationCode`. */
export function defineApplication(applicationCode) {
    return new DefinitionSetBuilder(applicationCode);
}
