<?php

declare(strict_types=1);

namespace FlowCatalyst\Sync;

/**
 * Options for the sync operation.
 */
final class SyncOptions
{
    public function __construct(
        /**
         * Remove definitions from the platform that are not in the local set.
         * Only applies to API-sourced definitions (won't remove UI-created ones).
         */
        public readonly bool $removeUnlisted = false,

        /**
         * Whether to sync roles.
         */
        public readonly bool $syncRoles = true,

        /**
         * Whether to sync event types.
         */
        public readonly bool $syncEventTypes = true,

        /**
         * Whether to sync subscriptions.
         */
        public readonly bool $syncSubscriptions = true,

        /**
         * Whether to sync dispatch pools.
         */
        public readonly bool $syncDispatchPools = true,

        /**
         * Whether to sync principals (users with roles).
         */
        public readonly bool $syncPrincipals = true,

        /**
         * Whether to sync processes (workflow documentation).
         */
        public readonly bool $syncProcesses = true,

        /**
         * Whether to sync scheduled jobs (cron-driven background work).
         */
        public readonly bool $syncScheduledJobs = true,

        /**
         * Whether to publish the OpenAPI spec attached to the
         * definition set (single-document; replaces prior version).
         */
        public readonly bool $syncOpenapi = true,

        /**
         * Whether to sync connections. Always applied BEFORE subscriptions —
         * a subscription's `connectionCode` must resolve in the same run.
         * Trailing (not grouped next to syncSubscriptions above) so this
         * stays an additive, backward-compatible constructor change.
         */
        public readonly bool $syncConnections = true,
    ) {}

    /**
     * Create default options.
     */
    public static function defaults(): self
    {
        return new self();
    }

    /**
     * Create options that remove unlisted definitions.
     */
    public static function withRemoveUnlisted(): self
    {
        return new self(removeUnlisted: true);
    }

    /**
     * Create options that only sync roles.
     */
    public static function rolesOnly(): self
    {
        return new self(
            syncRoles: true,
            syncEventTypes: false,
            syncSubscriptions: false,
            syncConnections: false,
            syncDispatchPools: false,
            syncPrincipals: false,
            syncProcesses: false,
            syncScheduledJobs: false,
            syncOpenapi: false,
        );
    }

    /**
     * Create options that only sync event types.
     */
    public static function eventTypesOnly(): self
    {
        return new self(
            syncRoles: false,
            syncEventTypes: true,
            syncSubscriptions: false,
            syncConnections: false,
            syncDispatchPools: false,
            syncPrincipals: false,
            syncProcesses: false,
            syncScheduledJobs: false,
            syncOpenapi: false,
        );
    }

    /**
     * Create options that only sync subscriptions.
     */
    public static function subscriptionsOnly(): self
    {
        return new self(
            syncRoles: false,
            syncEventTypes: false,
            syncSubscriptions: true,
            syncConnections: false,
            syncDispatchPools: false,
            syncPrincipals: false,
            syncProcesses: false,
            syncScheduledJobs: false,
            syncOpenapi: false,
        );
    }

    /**
     * Create options that only sync connections.
     */
    public static function connectionsOnly(): self
    {
        return new self(
            syncRoles: false,
            syncEventTypes: false,
            syncSubscriptions: false,
            syncConnections: true,
            syncDispatchPools: false,
            syncPrincipals: false,
            syncProcesses: false,
            syncScheduledJobs: false,
            syncOpenapi: false,
        );
    }

    /**
     * Create options that only sync dispatch pools.
     */
    public static function dispatchPoolsOnly(): self
    {
        return new self(
            syncRoles: false,
            syncEventTypes: false,
            syncSubscriptions: false,
            syncConnections: false,
            syncDispatchPools: true,
            syncPrincipals: false,
            syncProcesses: false,
            syncScheduledJobs: false,
            syncOpenapi: false,
        );
    }

    /**
     * Create options that only sync principals.
     */
    public static function principalsOnly(): self
    {
        return new self(
            syncRoles: false,
            syncEventTypes: false,
            syncSubscriptions: false,
            syncConnections: false,
            syncDispatchPools: false,
            syncPrincipals: true,
            syncProcesses: false,
            syncScheduledJobs: false,
            syncOpenapi: false,
        );
    }

    /**
     * Create options that only sync processes.
     */
    public static function processesOnly(): self
    {
        return new self(
            syncRoles: false,
            syncEventTypes: false,
            syncSubscriptions: false,
            syncConnections: false,
            syncDispatchPools: false,
            syncPrincipals: false,
            syncProcesses: true,
            syncScheduledJobs: false,
            syncOpenapi: false,
        );
    }

    /**
     * Create options that only sync scheduled jobs.
     */
    public static function scheduledJobsOnly(): self
    {
        return new self(
            syncRoles: false,
            syncEventTypes: false,
            syncSubscriptions: false,
            syncConnections: false,
            syncDispatchPools: false,
            syncPrincipals: false,
            syncProcesses: false,
            syncScheduledJobs: true,
            syncOpenapi: false,
        );
    }

    /**
     * Create options that only publish the attached OpenAPI document.
     */
    public static function openapiOnly(): self
    {
        return new self(
            syncRoles: false,
            syncEventTypes: false,
            syncSubscriptions: false,
            syncConnections: false,
            syncDispatchPools: false,
            syncPrincipals: false,
            syncProcesses: false,
            syncScheduledJobs: false,
            syncOpenapi: true,
        );
    }

    /**
     * Create a copy with removeUnlisted enabled.
     */
    public function withRemoveUnlistedEnabled(): self
    {
        return new self(
            removeUnlisted: true,
            syncRoles: $this->syncRoles,
            syncEventTypes: $this->syncEventTypes,
            syncSubscriptions: $this->syncSubscriptions,
            syncConnections: $this->syncConnections,
            syncDispatchPools: $this->syncDispatchPools,
            syncPrincipals: $this->syncPrincipals,
            syncProcesses: $this->syncProcesses,
            syncScheduledJobs: $this->syncScheduledJobs,
            syncOpenapi: $this->syncOpenapi,
        );
    }
}
