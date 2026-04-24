/**
 * Syncing FlowCatalyst definitions — roles, event types, subscriptions,
 * dispatch pools, principals.
 *
 * Typical usage:
 *
 * ```ts
 * import { FlowCatalystClient, sync } from "@flowcatalyst/sdk";
 *
 * const client = new FlowCatalystClient({ ... });
 *
 * const definitions = sync
 *   .defineApplication("orders")
 *   .withRoles([{ name: "admin", displayName: "Administrator" }])
 *   .withEventTypes([
 *     { code: "orders:fulfillment:shipment:shipped", name: "Shipment Shipped" },
 *   ])
 *   .build();
 *
 * const result = await client.definitions().sync(definitions);
 * ```
 *
 * See `docs/syncing-definitions.md` for structure conventions (role names,
 * permission format, event-type codes, subscription modes, etc).
 */
export { defineApplication, DefinitionSetBuilder, } from "./definitions";
export { DefinitionSynchronizer, } from "./definition-synchronizer";
export { isSynced, SKIPPED, } from "./result";
