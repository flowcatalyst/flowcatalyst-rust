<?php

declare(strict_types=1);

namespace FlowCatalyst\Sync;

/**
 * Container for definitions to sync for a single FlowCatalyst application.
 *
 * This class allows you to build a complete set of definitions for one
 * application and sync them all at once. Definitions can be provided as
 * DTOs (RoleDefinition, EventTypeDefinition, SubscriptionDefinition) or
 * as plain arrays.
 *
 * Example usage:
 *
 * ```php
 * // Using DTOs
 * $definitions = SyncDefinitionSet::forApplication('my-app')
 *     ->withRoles([
 *         new RoleDefinition('admin', 'Administrator', permissions: ['user:manage']),
 *         new RoleDefinition('editor', 'Editor', permissions: ['content:edit']),
 *     ])
 *     ->withEventTypes([
 *         new EventTypeDefinition('user.created', 'User Created'),
 *     ]);
 *
 * // Using arrays
 * $definitions = SyncDefinitionSet::forApplication('my-app')
 *     ->withRoles([
 *         ['name' => 'admin', 'displayName' => 'Administrator'],
 *     ]);
 *
 * // Sync
 * $synchronizer->sync($definitions);
 * ```
 */
final class SyncDefinitionSet
{
    /** @var array<RoleDefinition|array<string, mixed>> */
    private array $roles = [];

    /**
     * Standalone permission definitions ({permission, description?}). Not pushed
     * to FlowCatalyst directly (permissions ride up via the roles that grant
     * them); used for local Spatie seeding and scan/dry-run output.
     *
     * @var array<array<string, mixed>>
     */
    private array $permissions = [];

    /** @var array<EventTypeDefinition|array<string, mixed>> */
    private array $eventTypes = [];

    /** @var array<SubscriptionDefinition|array<string, mixed>> */
    private array $subscriptions = [];

    /** @var array<ConnectionDefinition|array<string, mixed>> */
    private array $connections = [];

    /**
     * The FlowCatalyst client (identifier slug) this whole set is scoped to.
     * Null = global (the default, set via `forApplication()`). Set via
     * `forClient()` for a multi-tenant application's per-client set.
     */
    private ?string $client = null;

    /**
     * Overrides the base URL a subscription's path-style target is resolved
     * against, for THIS set only — a client-scoped set often has its own
     * host. Null = fall back to the synchronizer's configured default
     * (`flowcatalyst.subscriptions.target_base_url` / `app.url`).
     */
    private ?string $targetBaseUrl = null;

    /** @var array<DispatchPoolDefinition|array<string, mixed>> */
    private array $dispatchPools = [];

    /** @var array<PrincipalDefinition|array<string, mixed>> */
    private array $principals = [];

    /** @var array<ScheduledJobDefinition|array<string, mixed>> */
    private array $scheduledJobs = [];

    /** OpenAPI document (parsed JSON) — single value, not a list. */
    private mixed $openapiSpec = null;

    /** @var array<ProcessDefinition|array<string, mixed>> */
    private array $processes = [];

    public function __construct(
        public readonly string $applicationCode,
    ) {}

    /**
     * Create a new definition set for an application.
     */
    public static function forApplication(string $applicationCode): self
    {
        return new self($applicationCode);
    }

    /**
     * Scope this set to a specific FlowCatalyst client (identifier slug).
     * For a multi-tenant application, build one set per (application,
     * client) — the global set from `forApplication()` alone, and one per
     * tenant via `forApplication(...)->forClient(...)`.
     *
     * @param string $client Client identifier slug (never an id — ids differ
     *        per environment)
     * @param string|null $targetBaseUrl Overrides the base URL a
     *        subscription's path-style target resolves against, for this
     *        set only (tenants often have their own host). Null keeps the
     *        synchronizer's configured default.
     */
    public function forClient(string $client, ?string $targetBaseUrl = null): self
    {
        $clone = clone $this;
        $clone->client = $client;
        $clone->targetBaseUrl = $targetBaseUrl;
        return $clone;
    }

    /**
     * The client (identifier slug) this set is scoped to, or null when it is
     * the global set.
     */
    public function getClient(): ?string
    {
        return $this->client;
    }

    /**
     * Whether this is the global (client-less) set.
     */
    public function isGlobal(): bool
    {
        return $this->client === null;
    }

    /**
     * The base URL override for this set's subscription targets, or null to
     * use the synchronizer's configured default.
     */
    public function getTargetBaseUrl(): ?string
    {
        return $this->targetBaseUrl;
    }

    /**
     * Merge another set for the SAME application into a new combined set.
     *
     * The platform scopes `removeUnlisted` to one (application, client) PER
     * CALL — everything of that scope not present in the call is deleted.
     * Two sets that both target application X (even if one is "the scanned
     * attribute definitions" and the other "what a multi-tenant provider
     * yielded") must therefore become ONE call per (application, client),
     * never two — splitting one scope across two calls would let the second
     * call delete what the first just created. `DefinitionSynchronizer::
     * syncGrouped()` uses this to fold every set sharing an application code
     * into one before syncing.
     *
     * - Roles, permissions, event types, dispatch pools, principals and
     *   processes have no per-row client — they're simply concatenated.
     * - Connections and subscriptions ARE client-scoped per row: before
     *   concatenating, each row is stamped with its EFFECTIVE client (its
     *   own `client`, else the OWNING set's client) so the merged rows keep
     *   resolving to the right platform call once the combined set's own
     *   client is cleared below. Subscription rows are additionally stamped
     *   with the owning set's `targetBaseUrl` under the internal
     *   `_targetBaseUrl` key (never a real payload field — there is no such
     *   key on `SyncSubscriptionEntry` — purely so a per-set base URL
     *   survives the merge for target resolution).
     * - Scheduled jobs are concatenated as-is, WITHOUT stamping a set's
     *   client onto a job row that lacks one: `clientId` on a scheduled job
     *   is a deliberately independent, explicit-only axis (see
     *   `AsScheduledJob`'s docs) that a plain single-set `sync()` has never
     *   defaulted from the set's own client — merging must not introduce
     *   that inconsistency. `syncScheduledJobs()` already groups by each
     *   row's own `clientId`, so concatenation alone fixes the same
     *   cross-call clobbering for jobs sharing one (application, clientId).
     * - The combined set's own `client`/`targetBaseUrl` are always cleared:
     *   every row now carries what it needs, and leaving the set-level ones
     *   set would double-apply — or wrongly override — a row's own scope.
     * - Duplicate codes ending up in the same (application, client) scope
     *   after merging is a configuration error the SYNCHRONIZER catches
     *   (per type, per scope) before calling the platform — not this method.
     */
    public function merge(self $other): self
    {
        $clone = clone $this;
        $clone->client = null;
        $clone->targetBaseUrl = null;

        $clone->roles = [...$this->getRoles(), ...$other->getRoles()];
        $clone->permissions = [...$this->getPermissions(), ...$other->getPermissions()];
        $clone->eventTypes = [...$this->getEventTypes(), ...$other->getEventTypes()];
        $clone->dispatchPools = [...$this->getDispatchPools(), ...$other->getDispatchPools()];
        $clone->principals = [...$this->getPrincipals(), ...$other->getPrincipals()];
        $clone->processes = [...$this->getProcesses(), ...$other->getProcesses()];
        $clone->scheduledJobs = [...$this->getScheduledJobs(), ...$other->getScheduledJobs()];
        $clone->connections = [...$this->stampedConnections(), ...$other->stampedConnections()];
        $clone->subscriptions = [...$this->stampedSubscriptions(), ...$other->stampedSubscriptions()];

        // Only one OpenAPI document makes sense per application; keep
        // whichever side actually attached one (first wins — arbitrary, but
        // deterministic, and no caller has attached one to more than one set
        // of the same application in practice).
        $clone->openapiSpec = $this->openapiSpec ?? $other->openapiSpec;

        return $clone;
    }

    /**
     * This set's connections, each row stamped with its EFFECTIVE client —
     * its own `client` if set, else this set's client. Used by `merge()` so
     * a row still resolves to the right platform call after the owning
     * set's own client is cleared.
     *
     * @return array<array<string, mixed>>
     */
    private function stampedConnections(): array
    {
        return array_map(fn(array $row) => $this->stampRowClient($row), $this->getConnections());
    }

    /**
     * This set's subscriptions, each row stamped with its effective client
     * (see {@see stampedConnections()}) AND, for target resolution, this
     * set's `targetBaseUrl` under the internal `_targetBaseUrl` key — unless
     * the row already carries one (e.g. it survived an earlier merge step).
     *
     * @return array<array<string, mixed>>
     */
    private function stampedSubscriptions(): array
    {
        return array_map(function (array $row) {
            $row = $this->stampRowClient($row);
            if (!array_key_exists('_targetBaseUrl', $row) || $row['_targetBaseUrl'] === null) {
                $row['_targetBaseUrl'] = $this->targetBaseUrl;
            }
            return $row;
        }, $this->getSubscriptions());
    }

    /**
     * @param array<string, mixed> $row
     * @return array<string, mixed>
     */
    private function stampRowClient(array $row): array
    {
        $rowClient = $row['client'] ?? null;
        $row['client'] = (is_string($rowClient) && $rowClient !== '') ? $rowClient : $this->client;
        return $row;
    }

    /**
     * Add roles to the definition set.
     *
     * @param array<RoleDefinition|array<string, mixed>> $roles
     */
    public function withRoles(array $roles): self
    {
        $clone = clone $this;
        $clone->roles = $roles;
        return $clone;
    }

    /**
     * Add a single role to the definition set.
     *
     * @param RoleDefinition|array<string, mixed> $role
     */
    public function addRole(RoleDefinition|array $role): self
    {
        $clone = clone $this;
        $clone->roles = [...$this->roles, $role];
        return $clone;
    }

    /**
     * Add event types to the definition set.
     *
     * @param array<EventTypeDefinition|array<string, mixed>> $eventTypes
     */
    public function withEventTypes(array $eventTypes): self
    {
        $clone = clone $this;
        $clone->eventTypes = $eventTypes;
        return $clone;
    }

    /**
     * Add a single event type to the definition set.
     *
     * @param EventTypeDefinition|array<string, mixed> $eventType
     */
    public function addEventType(EventTypeDefinition|array $eventType): self
    {
        $clone = clone $this;
        $clone->eventTypes = [...$this->eventTypes, $eventType];
        return $clone;
    }

    /**
     * Add subscriptions to the definition set.
     *
     * @param array<SubscriptionDefinition|array<string, mixed>> $subscriptions
     */
    public function withSubscriptions(array $subscriptions): self
    {
        $clone = clone $this;
        $clone->subscriptions = $subscriptions;
        return $clone;
    }

    /**
     * Add a single subscription to the definition set.
     *
     * @param SubscriptionDefinition|array<string, mixed> $subscription
     */
    public function addSubscription(SubscriptionDefinition|array $subscription): self
    {
        $clone = clone $this;
        $clone->subscriptions = [...$this->subscriptions, $subscription];
        return $clone;
    }

    /**
     * Add connections to the definition set. Synced BEFORE subscriptions —
     * a subscription's `connectionCode` must resolve in the same run.
     *
     * @param array<ConnectionDefinition|array<string, mixed>> $connections
     */
    public function withConnections(array $connections): self
    {
        $clone = clone $this;
        $clone->connections = $connections;
        return $clone;
    }

    /**
     * Add a single connection to the definition set.
     *
     * @param ConnectionDefinition|array<string, mixed> $connection
     */
    public function addConnection(ConnectionDefinition|array $connection): self
    {
        $clone = clone $this;
        $clone->connections = [...$this->connections, $connection];
        return $clone;
    }

    /**
     * Get connections as arrays for the sync API.
     *
     * @return array<array<string, mixed>>
     */
    public function getConnections(): array
    {
        return array_map(
            fn($c) => $c instanceof ConnectionDefinition ? $c->toArray() : $c,
            $this->connections
        );
    }

    /**
     * Check if there are any connections to sync.
     */
    public function hasConnections(): bool
    {
        return !empty($this->connections);
    }

    /**
     * Add dispatch pools to the definition set.
     *
     * @param array<DispatchPoolDefinition|array<string, mixed>> $dispatchPools
     */
    public function withDispatchPools(array $dispatchPools): self
    {
        $clone = clone $this;
        $clone->dispatchPools = $dispatchPools;
        return $clone;
    }

    /**
     * Add a single dispatch pool to the definition set.
     *
     * @param DispatchPoolDefinition|array<string, mixed> $dispatchPool
     */
    public function addDispatchPool(DispatchPoolDefinition|array $dispatchPool): self
    {
        $clone = clone $this;
        $clone->dispatchPools = [...$this->dispatchPools, $dispatchPool];
        return $clone;
    }

    /**
     * Get roles as arrays for the sync API.
     *
     * @return array<array<string, mixed>>
     */
    public function getRoles(): array
    {
        return array_map(
            fn($role) => $role instanceof RoleDefinition ? $role->toArray() : $role,
            $this->roles
        );
    }

    /**
     * Add standalone permission definitions.
     *
     * @param array<array<string, mixed>> $permissions Each {permission, description?}
     */
    public function withPermissions(array $permissions): self
    {
        $clone = clone $this;
        $clone->permissions = $permissions;
        return $clone;
    }

    /**
     * Get the standalone permission definitions.
     *
     * @return array<array<string, mixed>>
     */
    public function getPermissions(): array
    {
        return $this->permissions;
    }

    /**
     * Check if there are any standalone permission definitions.
     */
    public function hasPermissions(): bool
    {
        return !empty($this->permissions);
    }

    /**
     * Get event types as arrays for the sync API.
     *
     * @return array<array<string, mixed>>
     */
    public function getEventTypes(): array
    {
        return array_map(
            fn($et) => $et instanceof EventTypeDefinition ? $et->toArray() : $et,
            $this->eventTypes
        );
    }

    /**
     * Get subscriptions as arrays for the sync API.
     *
     * @return array<array<string, mixed>>
     */
    public function getSubscriptions(): array
    {
        return array_map(
            fn($sub) => $sub instanceof SubscriptionDefinition ? $sub->toArray() : $sub,
            $this->subscriptions
        );
    }

    /**
     * Get dispatch pools as arrays for the sync API.
     *
     * @return array<array<string, mixed>>
     */
    public function getDispatchPools(): array
    {
        return array_map(
            fn($pool) => $pool instanceof DispatchPoolDefinition ? $pool->toArray() : $pool,
            $this->dispatchPools
        );
    }

    /**
     * Add principals to the definition set.
     *
     * @param array<PrincipalDefinition|array<string, mixed>> $principals
     */
    public function withPrincipals(array $principals): self
    {
        $clone = clone $this;
        $clone->principals = $principals;
        return $clone;
    }

    /**
     * Add a single principal to the definition set.
     *
     * @param PrincipalDefinition|array<string, mixed> $principal
     */
    public function addPrincipal(PrincipalDefinition|array $principal): self
    {
        $clone = clone $this;
        $clone->principals = [...$this->principals, $principal];
        return $clone;
    }

    /**
     * Get principals as arrays for the sync API.
     *
     * @return array<array<string, mixed>>
     */
    public function getPrincipals(): array
    {
        return array_map(
            fn($p) => $p instanceof PrincipalDefinition ? $p->toArray() : $p,
            $this->principals
        );
    }

    /**
     * Check if there are any principals to sync.
     */
    public function hasPrincipals(): bool
    {
        return !empty($this->principals);
    }

    /**
     * Add processes to the definition set.
     *
     * @param array<ProcessDefinition|array<string, mixed>> $processes
     */
    public function withProcesses(array $processes): self
    {
        $clone = clone $this;
        $clone->processes = $processes;
        return $clone;
    }

    /**
     * Add a single process to the definition set.
     *
     * @param ProcessDefinition|array<string, mixed> $process
     */
    public function addProcess(ProcessDefinition|array $process): self
    {
        $clone = clone $this;
        $clone->processes = [...$this->processes, $process];
        return $clone;
    }

    /**
     * Get processes as arrays for the sync API.
     *
     * @return array<array<string, mixed>>
     */
    public function getProcesses(): array
    {
        return array_map(
            fn($p) => $p instanceof ProcessDefinition ? $p->toArray() : $p,
            $this->processes
        );
    }

    /**
     * Check if there are any processes to sync.
     */
    public function hasProcesses(): bool
    {
        return !empty($this->processes);
    }

    /**
     * Add scheduled jobs to the definition set.
     *
     * @param array<ScheduledJobDefinition|array<string, mixed>> $scheduledJobs
     */
    public function withScheduledJobs(array $scheduledJobs): self
    {
        $clone = clone $this;
        $clone->scheduledJobs = $scheduledJobs;
        return $clone;
    }

    /**
     * Add a single scheduled job to the definition set.
     *
     * @param ScheduledJobDefinition|array<string, mixed> $job
     */
    public function addScheduledJob(ScheduledJobDefinition|array $job): self
    {
        $clone = clone $this;
        $clone->scheduledJobs = [...$this->scheduledJobs, $job];
        return $clone;
    }

    /**
     * Get scheduled jobs as arrays for the sync API.
     *
     * @return array<array<string, mixed>>
     */
    public function getScheduledJobs(): array
    {
        return array_map(
            fn($j) => $j instanceof ScheduledJobDefinition ? $j->toArray() : $j,
            $this->scheduledJobs
        );
    }

    public function hasScheduledJobs(): bool
    {
        return !empty($this->scheduledJobs);
    }

    /**
     * Attach an OpenAPI document (parsed JSON) to publish on next sync.
     */
    public function withOpenapiSpec(mixed $spec): self
    {
        $clone = clone $this;
        $clone->openapiSpec = $spec;
        return $clone;
    }

    public function getOpenapiSpec(): mixed
    {
        return $this->openapiSpec;
    }

    public function hasOpenapiSpec(): bool
    {
        return $this->openapiSpec !== null;
    }

    /**
     * Check if there are any roles to sync.
     */
    public function hasRoles(): bool
    {
        return !empty($this->roles);
    }

    /**
     * Check if there are any event types to sync.
     */
    public function hasEventTypes(): bool
    {
        return !empty($this->eventTypes);
    }

    /**
     * Check if there are any subscriptions to sync.
     */
    public function hasSubscriptions(): bool
    {
        return !empty($this->subscriptions);
    }

    /**
     * Check if there are any dispatch pools to sync.
     */
    public function hasDispatchPools(): bool
    {
        return !empty($this->dispatchPools);
    }

    /**
     * Check if the definition set is empty.
     */
    public function isEmpty(): bool
    {
        return !$this->hasRoles()
            && !$this->hasPermissions()
            && !$this->hasEventTypes()
            && !$this->hasSubscriptions()
            && !$this->hasConnections()
            && !$this->hasDispatchPools()
            && !$this->hasPrincipals()
            && !$this->hasProcesses()
            && !$this->hasScheduledJobs()
            && !$this->hasOpenapiSpec();
    }

    /**
     * Create from the scanned definitions repository format.
     *
     * @param string $applicationCode
     * @param array{roles?: array<array<string, mixed>>, eventTypes?: array<array<string, mixed>>, subscriptions?: array<array<string, mixed>>, connections?: array<array<string, mixed>>, dispatchPools?: array<array<string, mixed>>, principals?: array<array<string, mixed>>, processes?: array<array<string, mixed>>} $data
     */
    public static function fromScannedDefinitions(string $applicationCode, array $data): self
    {
        $set = new self($applicationCode);

        // Remove internal scanner-only fields (_class, _application) so they
        // never leak into the synced payload.
        $strip = static function (array $def): array {
            unset($def['_class'], $def['_application']);
            return $def;
        };

        $set->roles = array_map($strip, $data['roles'] ?? []);
        $set->permissions = array_map($strip, $data['permissions'] ?? []);
        $set->eventTypes = array_map($strip, $data['eventTypes'] ?? []);
        $set->subscriptions = array_map($strip, $data['subscriptions'] ?? []);
        $set->connections = array_map($strip, $data['connections'] ?? []);
        $set->dispatchPools = array_map($strip, $data['dispatchPools'] ?? []);
        $set->principals = array_map($strip, $data['principals'] ?? []);
        $set->processes = array_map($strip, $data['processes'] ?? []);
        $set->scheduledJobs = array_map($strip, $data['scheduledJobs'] ?? []);

        if (isset($data['openapiSpec'])) {
            $set->openapiSpec = $data['openapiSpec'];
        }

        return $set;
    }
}
