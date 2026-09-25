<?php

declare(strict_types=1);

namespace FlowCatalyst\Sync;

/**
 * Implemented by an app-owned class that supplies definition sets to
 * `flowcatalyst:sync` beyond what the attribute scanner finds — the
 * multi-tenant path. Attributes are compiled into the codebase, but a
 * multi-tenant application's client list is runtime data (a database table,
 * a config file, …) that no attribute could express. Such an application
 * builds one `SyncDefinitionSet` per (application, client) programmatically
 * instead — the global set from `SyncDefinitionSet::forApplication(...)`,
 * and one per tenant via `->forClient(...)`.
 *
 * Register your implementation(s) in `flowcatalyst.definitions.set_providers`
 * (class names, resolved through the container); `flowcatalyst:sync` resolves
 * each and syncs every set it yields, IN ADDITION to the scanned attribute
 * definitions.
 */
interface ProvidesSyncDefinitionSets
{
    /**
     * @return iterable<SyncDefinitionSet>
     */
    public function syncDefinitionSets(): iterable;
}
