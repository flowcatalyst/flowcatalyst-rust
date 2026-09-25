/**
 * Syncing FlowCatalyst definitions — roles, event types, connections,
 * subscriptions, dispatch pools, principals.
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

export {
	APP_CODE_ENV,
	defineApplication,
	defineApplicationFromEnv,
	DefinitionSetBuilder,
	permission,
	permissionToString,
	type DefinitionSet,
	type RoleDefinition,
	type PermissionInput,
	type EventTypeDefinition,
	type ConnectionDefinition,
	type SubscriptionDefinition,
	type SubscriptionEventTypeBinding,
	type SubscriptionMode,
	type DispatchPoolDefinition,
	type PrincipalDefinition,
	type ProcessDefinition,
	type ScheduledJobDefinition,
} from "./definitions.js";

export {
	DefinitionSynchronizer,
	type SyncOptions,
	type DefinitionSynchronizerOptions,
	type DefinitionSyncError,
} from "./definition-synchronizer.js";

export {
	isSynced,
	SKIPPED,
	type SyncResult,
	type CategorySyncResult,
	type SkippedSyncResult,
	type MaybeCategoryResult,
} from "./result.js";
