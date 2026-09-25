<?php

declare(strict_types=1);

namespace FlowCatalyst\Sync;

use FlowCatalyst\Client\FlowCatalystClient;
use FlowCatalyst\DTOs\EventTypeBinding;
use FlowCatalyst\DTOs\Requests\SyncConnectionEntry;
use FlowCatalyst\DTOs\Requests\SyncDispatchPoolEntry;
use FlowCatalyst\DTOs\Requests\SyncEventTypeEntry;
use FlowCatalyst\DTOs\Requests\SyncPrincipalEntry;
use FlowCatalyst\DTOs\Requests\SyncProcessEntry;
use FlowCatalyst\DTOs\Requests\SyncRoleEntry;
use FlowCatalyst\DTOs\Requests\SyncScheduledJobEntry;
use FlowCatalyst\DTOs\Requests\SyncSubscriptionEntry;

/**
 * Service for synchronizing FlowCatalyst definitions to the platform.
 *
 * This service provides a programmatic API for syncing definitions without
 * requiring the use of PHP attributes or the definition scanner. It supports
 * syncing multiple applications from a single deployment.
 *
 * Example usage:
 *
 * ```php
 * // Single application sync
 * $synchronizer = app(DefinitionSynchronizer::class);
 *
 * $result = $synchronizer->sync(
 *     SyncDefinitionSet::forApplication('my-app')
 *         ->withRoles([
 *             new RoleDefinition('admin', 'Administrator'),
 *         ])
 *         ->withEventTypes([
 *             new EventTypeDefinition('user.created', 'User Created'),
 *         ])
 * );
 *
 * // Multi-application sync
 * $results = $synchronizer->syncAll([
 *     SyncDefinitionSet::forApplication('app-one')
 *         ->withEventTypes([...]),
 *     SyncDefinitionSet::forApplication('app-two')
 *         ->withEventTypes([...]),
 * ], SyncOptions::withRemoveUnlisted());
 * ```
 */
class DefinitionSynchronizer
{
    public function __construct(
        private readonly FlowCatalystClient $client,
        // Fallback callback URL for scheduled jobs whose #[AsScheduledJob]
        // doesn't set targetUrl. The platform stores targetUrl verbatim (no
        // server-side default) and a job without one fails EVERY firing with
        // "No target URL configured" — so the sync must always send one. The
        // service provider passes APP_URL + ScheduledJobRunner's conventional
        // process path.
        private readonly ?string $defaultScheduledJobTargetUrl = null,
        // Base URL a subscription's path-style target (`/webhooks/orders`) is
        // resolved against. The service provider passes
        // `flowcatalyst.subscriptions.target_base_url`, falling back to
        // `app.url`. Resolved HERE rather than at scan time because the scan
        // cache may be built in CI, where the deploying environment's host
        // isn't known yet.
        private readonly ?string $subscriptionTargetBaseUrl = null,
    ) {}

    /**
     * Sync definitions for a single application.
     *
     * @param SyncDefinitionSet $definitions The definitions to sync
     * @param SyncOptions|null $options Sync options (defaults to SyncOptions::defaults())
     * @return SyncResult The sync results
     */
    public function sync(SyncDefinitionSet $definitions, ?SyncOptions $options = null): SyncResult
    {
        $options ??= SyncOptions::defaults();
        $appCode = $definitions->applicationCode;

        $rolesResult = ['created' => 0, 'updated' => 0, 'deleted' => 0];
        $eventTypesResult = ['created' => 0, 'updated' => 0, 'deleted' => 0];
        $subscriptionsResult = ['created' => 0, 'updated' => 0, 'deleted' => 0];
        $connectionsResult = ['created' => 0, 'updated' => 0, 'deleted' => 0];
        $dispatchPoolsResult = ['created' => 0, 'updated' => 0, 'deleted' => 0];
        $principalsResult = ['created' => 0, 'updated' => 0, 'deleted' => 0];
        $processesResult = ['created' => 0, 'updated' => 0, 'deleted' => 0];
        $scheduledJobsResult = ['created' => 0, 'updated' => 0, 'deleted' => 0];
        $openapiResult = ['created' => 0, 'updated' => 0, 'deleted' => 0];

        // Sync roles
        if ($options->syncRoles && $definitions->hasRoles()) {
            $rolesResult = $this->syncRoles($appCode, $definitions->getRoles(), $options->removeUnlisted);
        }

        // Sync event types
        if ($options->syncEventTypes && $definitions->hasEventTypes()) {
            $eventTypesResult = $this->syncEventTypes($appCode, $definitions->getEventTypes(), $options->removeUnlisted);
        }

        // Sync connections, then subscriptions — grouped by resolved client
        // (one platform call per distinct client per resource), global
        // before client, connections before subscriptions within each
        // client group. See syncConnectionsAndSubscriptions() for why both
        // are handled together.
        [$connectionsResult, $subscriptionsResult] = $this->syncConnectionsAndSubscriptions($appCode, $definitions, $options);

        // Sync dispatch pools
        if ($options->syncDispatchPools && $definitions->hasDispatchPools()) {
            $dispatchPoolsResult = $this->syncDispatchPools($appCode, $definitions->getDispatchPools(), $options->removeUnlisted);
        }

        // Sync principals (users with roles)
        if ($options->syncPrincipals && $definitions->hasPrincipals()) {
            $principalsResult = $this->syncPrincipals($appCode, $definitions->getPrincipals(), $options->removeUnlisted);
        }

        // Sync processes (workflow documentation)
        if ($options->syncProcesses && $definitions->hasProcesses()) {
            $processesResult = $this->syncProcesses($appCode, $definitions->getProcesses(), $options->removeUnlisted);
        }

        // Sync scheduled jobs
        if ($options->syncScheduledJobs && $definitions->hasScheduledJobs()) {
            $scheduledJobsResult = $this->syncScheduledJobs($appCode, $definitions->getScheduledJobs(), $options->removeUnlisted);
        }

        // Publish attached OpenAPI document
        if ($options->syncOpenapi && $definitions->hasOpenapiSpec()) {
            $openapiResult = $this->syncOpenapi($appCode, $definitions->getOpenapiSpec());
        }

        return new SyncResult(
            applicationCode: $appCode,
            roles: $rolesResult,
            eventTypes: $eventTypesResult,
            subscriptions: $subscriptionsResult,
            dispatchPools: $dispatchPoolsResult,
            principals: $principalsResult,
            processes: $processesResult,
            scheduledJobs: $scheduledJobsResult,
            openapi: $openapiResult,
            connections: $connectionsResult,
        );
    }

    /**
     * Sync multiple definition sets, grouping by application code and
     * MERGING every set that shares one into a single combined set — then
     * calling `sync()` exactly ONCE per application.
     *
     * This is not an optimisation: the platform scopes `removeUnlisted` to
     * one (application, client) PER CALL, so two sets for the same
     * application (e.g. the scanned attribute definitions plus a
     * multi-tenant provider's global set, or two providers each yielding a
     * set for the same client) must never become two separate calls for
     * that scope — the second would delete what the first just created.
     * {@see SyncDefinitionSet::merge()} does the actual folding (including
     * stamping each connection/subscription row with its effective client
     * so the existing per-row `groupByClient()` grouping in
     * `syncConnectionsAndSubscriptions()` still issues one call per
     * (application, client) — global first, as before — no matter how many
     * original sets a client's rows came from).
     *
     * @param iterable<SyncDefinitionSet> $definitionSets
     * @param SyncOptions|null $options Sync options applied to every set
     * @return array<string, SyncResult> keyed by application code
     */
    public function syncGrouped(iterable $definitionSets, ?SyncOptions $options = null): array
    {
        $byApp = [];
        foreach ($definitionSets as $set) {
            $byApp[$set->applicationCode][] = $set;
        }

        $results = [];
        foreach ($byApp as $appCode => $sets) {
            // Stable sort (PHP 8+): global sets (client === null) first,
            // client sets after — documents intent and keeps a single-set
            // application's set untouched; the actual global-before-client
            // CALL ordering after merging comes from groupByClient()'s own
            // sort in syncConnectionsAndSubscriptions().
            usort($sets, static fn(SyncDefinitionSet $a, SyncDefinitionSet $b) => (!$a->isGlobal()) <=> (!$b->isGlobal()));

            $combined = array_reduce(
                array_slice($sets, 1),
                static fn(SyncDefinitionSet $carry, SyncDefinitionSet $set) => $carry->merge($set),
                $sets[0],
            );

            $results[$appCode] = $this->sync($combined, $options);
        }

        return $results;
    }

    /**
     * Sync definitions for multiple applications.
     *
     * @param SyncDefinitionSet[] $definitionSets Array of definition sets, one per application
     * @param SyncOptions|null $options Sync options applied to all applications
     * @return SyncResult[] Array of sync results, keyed by application code
     */
    public function syncAll(array $definitionSets, ?SyncOptions $options = null): array
    {
        $results = [];

        foreach ($definitionSets as $definitions) {
            $results[$definitions->applicationCode] = $this->sync($definitions, $options);
        }

        return $results;
    }

    /**
     * Sync roles for an application.
     *
     * @param string $appCode Application code
     * @param array<array<string, mixed>> $roles Role definitions
     * @param bool $removeUnlisted Remove roles not in the local set
     * @return array{created: int, updated: int, deleted: int, error?: string}
     */
    private function syncRoles(string $appCode, array $roles, bool $removeUnlisted): array
    {
        // Validate role names before syncing
        $validationErrors = $this->validateRoles($roles);

        // Two sets for the same application (e.g. after syncGrouped() merges
        // them) defining the same role name is a configuration error, not
        // something to silently resolve by keeping whichever happened last.
        $duplicates = $this->findDuplicates(array_map(static fn($r) => (string) ($r['name'] ?? ''), $roles));
        if ($duplicates !== []) {
            $validationErrors[] = sprintf(
                'Duplicate role name(s) for %s: %s',
                $this->scopeLabel($appCode, null),
                implode(', ', $duplicates),
            );
        }

        if (!empty($validationErrors)) {
            return [
                'created' => 0,
                'updated' => 0,
                'deleted' => 0,
                'error' => implode('; ', $validationErrors),
            ];
        }

        try {
            $entries = array_map(
                fn(array $row) => new SyncRoleEntry(
                    name: (string) ($row['name'] ?? ''),
                    displayName: isset($row['displayName']) ? (string) $row['displayName'] : null,
                    description: isset($row['description']) ? (string) $row['description'] : null,
                    permissions: $row['permissions'] ?? [],
                    clientManaged: (bool) ($row['clientManaged'] ?? false),
                ),
                $roles,
            );
            $result = $this->client->roles()->sync($appCode, $entries, $removeUnlisted);

            return [
                'created' => $result->created,
                'updated' => $result->updated,
                'deleted' => $result->deleted,
            ];
        } catch (\Exception $e) {
            return [
                'created' => 0,
                'updated' => 0,
                'deleted' => 0,
                'error' => $e->getMessage(),
            ];
        }
    }

    /**
     * Validate role definitions before syncing.
     *
     * @param array<array<string, mixed>> $roles Role definitions
     * @return string[] Validation error messages
     */
    private function validateRoles(array $roles): array
    {
        $errors = [];

        foreach ($roles as $role) {
            $name = $role['name'] ?? '';
            $error = RoleDefinition::validateName($name);
            if ($error !== null) {
                $errors[] = $error;
            }
        }

        return $errors;
    }

    /**
     * Values that appear more than once in $values — a configuration error
     * when it happens (two definitions colliding in the same sync scope),
     * most commonly surfacing after `syncGrouped()` merges two otherwise
     * individually-valid sets for the same application (or application +
     * client) into one call. Blank values are ignored — already invalid on
     * their own terms, reported elsewhere.
     *
     * @param string[] $values
     * @return string[]
     */
    private function findDuplicates(array $values): array
    {
        $counts = array_count_values(array_filter($values, static fn($v) => is_string($v) && $v !== ''));
        return array_keys(array_filter($counts, static fn(int $c) => $c > 1));
    }

    /**
     * Human-readable label for a sync scope, used in duplicate-code error
     * messages.
     */
    private function scopeLabel(string $appCode, ?string $clientId): string
    {
        return $clientId === null
            ? "application \"{$appCode}\""
            : "application \"{$appCode}\", client \"{$clientId}\"";
    }

    /**
     * Sync event types for an application.
     *
     * @param string $appCode Application code
     * @param array<array<string, mixed>> $eventTypes Event type definitions
     * @param bool $removeUnlisted Remove event types not in the local set
     * @return array{created: int, updated: int, deleted: int, error?: string}
     */
    private function syncEventTypes(string $appCode, array $eventTypes, bool $removeUnlisted): array
    {
        $codes = array_map(fn(array $row) => $this->eventTypeCode($row, $appCode), $eventTypes);
        $duplicates = $this->findDuplicates($codes);
        if ($duplicates !== []) {
            return [
                'created' => 0,
                'updated' => 0,
                'deleted' => 0,
                'error' => sprintf(
                    'Duplicate event type code(s) for %s: %s',
                    $this->scopeLabel($appCode, null),
                    implode(', ', $duplicates),
                ),
            ];
        }

        try {
            $entries = array_map(
                fn(array $row, string $code) => new SyncEventTypeEntry(
                    code: $code,
                    name: (string) ($row['name'] ?? ''),
                    description: isset($row['description']) ? (string) $row['description'] : null,
                ),
                $eventTypes,
                $codes,
            );
            $result = $this->client->eventTypes()->sync($appCode, $entries, $removeUnlisted);

            return [
                'created' => $result->created,
                'updated' => $result->updated,
                'deleted' => $result->deleted,
            ];
        } catch (\Exception $e) {
            return [
                'created' => 0,
                'updated' => 0,
                'deleted' => 0,
                'error' => $e->getMessage(),
            ];
        }
    }

    /**
     * Resolve the full event-type code from a definition row.
     *
     * Prefers an explicit `code`; otherwise assembles it from the individual
     * {application,subdomain,aggregate,event} segments. This covers both
     * EventTypeDefinition::toArray() rows (which carry their own `application`)
     * and raw scanned `#[AsEventType]` rows (which DON'T — the application is
     * the set's $appCode, applied here). Prevents a missing segment from
     * silently becoming an empty code (which the API rejects with "must follow
     * format application:subdomain:aggregate:event").
     *
     * @param array<string, mixed> $row
     */
    private function eventTypeCode(array $row, string $appCode): string
    {
        $code = (string) ($row['code'] ?? '');
        if ($code !== '') {
            return $code;
        }

        $application = $row['application'] ?? ($appCode !== '' ? $appCode : null);
        $segments = [
            $application,
            $row['subdomain'] ?? null,
            $row['aggregate'] ?? null,
            $row['event'] ?? null,
        ];
        if (!in_array(null, $segments, true) && !in_array('', $segments, true)) {
            return implode(':', array_map('strval', $segments));
        }

        return $code;
    }

    /**
     * The absolute delivery URL for one subscription row, or null when it
     * cannot be determined. An absolute `target` (anything with a scheme) is
     * used verbatim; a path is joined onto the base URL. `endpoint` is
     * accepted as an alias — it is the platform's name for the same field on
     * the single-subscription create path.
     *
     * @param array<string, mixed> $row
     */
    private function resolveSubscriptionTarget(array $row, ?string $baseUrl): ?string
    {
        $target = trim((string) ($row['target'] ?? $row['endpoint'] ?? ''));
        if ($target === '') {
            return null;
        }
        if (preg_match('#^[a-z][a-z0-9+.\-]*://#i', $target) === 1) {
            return $target;
        }
        if ($baseUrl === null || trim($baseUrl) === '') {
            return null;
        }
        return rtrim(trim($baseUrl), '/') . '/' . ltrim($target, '/');
    }

    /**
     * Resolve the effective client (identifier slug) a scanned/programmatic
     * row belongs to: the row's own `client` (set by the attribute scanner,
     * or by a programmatic `ConnectionDefinition`/`SubscriptionDefinition`)
     * wins; otherwise the whole set's client (from `forClient()`) applies —
     * so a multi-tenant set's rows don't each need to repeat it. Returns ''
     * (not null) for the global group, so it can double as an array key.
     *
     * @param array<string, mixed> $row
     */
    private function resolveRowClient(array $row, ?string $setClient): string
    {
        $client = (isset($row['client']) && is_string($row['client']) && $row['client'] !== '')
            ? $row['client']
            : $setClient;

        return is_string($client) ? $client : '';
    }

    /**
     * Group connection/subscription rows by their effective client (see
     * {@see resolveRowClient()}). The platform's sync endpoints accept one
     * `clientId` per call, so each distinct client (including '' for
     * global) becomes its own request.
     *
     * @param array<array<string, mixed>> $rows
     * @return array<string, array<array<string, mixed>>>
     */
    private function groupByClient(array $rows, ?string $setClient): array
    {
        $groups = [];
        foreach ($rows as $row) {
            $groups[$this->resolveRowClient($row, $setClient)][] = $row;
        }
        return $groups;
    }

    /**
     * Sync connections, then subscriptions, for one definition set — grouped
     * by resolved client (one platform call per distinct client per
     * resource, mirroring {@see syncScheduledJobs()}'s clientId grouping).
     *
     * Ordering, per the platform's ownership model:
     *   - the global group ('' — no client) is processed before any
     *     client group, because a client-scoped subscription may reference
     *     a global connection;
     *   - within EACH group, connections are synced before subscriptions,
     *     because a subscription's `connectionCode` must resolve in the
     *     same run;
     *   - if a group's connection sync fails, that group's subscriptions
     *     are skipped entirely (their connection codes may not resolve) —
     *     recorded as an error rather than sent as a request that would
     *     404.
     *
     * @return array{0: array{created:int,updated:int,deleted:int,error?:string}, 1: array{created:int,updated:int,deleted:int,error?:string}}
     */
    private function syncConnectionsAndSubscriptions(string $appCode, SyncDefinitionSet $definitions, SyncOptions $options): array
    {
        $connectionsResult = ['created' => 0, 'updated' => 0, 'deleted' => 0];
        $subscriptionsResult = ['created' => 0, 'updated' => 0, 'deleted' => 0];

        $doConnections = $options->syncConnections && $definitions->hasConnections();
        $doSubscriptions = $options->syncSubscriptions && $definitions->hasSubscriptions();
        if (!$doConnections && !$doSubscriptions) {
            return [$connectionsResult, $subscriptionsResult];
        }

        $setClient = $definitions->getClient();
        $connectionGroups = $doConnections ? $this->groupByClient($definitions->getConnections(), $setClient) : [];
        $subscriptionGroups = $doSubscriptions ? $this->groupByClient($definitions->getSubscriptions(), $setClient) : [];

        $clientKeys = array_unique([...array_keys($connectionGroups), ...array_keys($subscriptionGroups)]);
        usort($clientKeys, static fn($a, $b) => ($a !== '') <=> ($b !== ''));

        $connErrors = [];
        $subErrors = [];
        $failedGroups = [];

        foreach ($clientKeys as $clientKey) {
            $clientId = $clientKey !== '' ? $clientKey : null;

            if (isset($connectionGroups[$clientKey])) {
                $result = $this->syncConnectionGroup($appCode, $connectionGroups[$clientKey], $clientId, $options->removeUnlisted);
                $connectionsResult['created'] += $result['created'];
                $connectionsResult['updated'] += $result['updated'];
                $connectionsResult['deleted'] += $result['deleted'];
                if (isset($result['error'])) {
                    $connErrors[] = $result['error'];
                    $failedGroups[$clientKey] = true;
                }
            }

            if (!isset($subscriptionGroups[$clientKey])) {
                continue;
            }

            if (isset($failedGroups[$clientKey])) {
                $subErrors[] = sprintf(
                    'Skipped subscription sync for %s: its connection sync failed first',
                    $clientKey === '' ? 'the global scope' : "client \"{$clientKey}\"",
                );
                continue;
            }

            $result = $this->syncSubscriptionGroup(
                $appCode,
                $subscriptionGroups[$clientKey],
                $clientId,
                $options->removeUnlisted,
                $definitions->getTargetBaseUrl() ?? $this->subscriptionTargetBaseUrl,
            );
            $subscriptionsResult['created'] += $result['created'];
            $subscriptionsResult['updated'] += $result['updated'];
            $subscriptionsResult['deleted'] += $result['deleted'];
            if (isset($result['error'])) {
                $subErrors[] = $result['error'];
            }
        }

        if ($connErrors !== []) {
            $connectionsResult['error'] = implode('; ', $connErrors);
        }
        if ($subErrors !== []) {
            $subscriptionsResult['error'] = implode('; ', $subErrors);
        }

        return [$connectionsResult, $subscriptionsResult];
    }

    /**
     * Sync one client-group's worth of connections.
     *
     * @param array<array<string, mixed>> $rows
     * @return array{created: int, updated: int, deleted: int, error?: string}
     */
    private function syncConnectionGroup(string $appCode, array $rows, ?string $clientId, bool $removeUnlisted): array
    {
        // Two sets contributing to the SAME (application, client) scope
        // (most commonly after syncGrouped() merges them) defining the same
        // connection code is a configuration error — fail locally, naming
        // the code and scope, rather than sending a request the platform
        // will reject or silently keeping whichever row happened to be last.
        $duplicates = $this->findDuplicates(array_map(static fn($r) => (string) ($r['code'] ?? ''), $rows));
        if ($duplicates !== []) {
            return [
                'created' => 0,
                'updated' => 0,
                'deleted' => 0,
                'error' => sprintf(
                    'Duplicate connection code(s) for %s: %s',
                    $this->scopeLabel($appCode, $clientId),
                    implode(', ', $duplicates),
                ),
            ];
        }

        try {
            $entries = array_map(
                fn(array $row) => new SyncConnectionEntry(
                    code: (string) ($row['code'] ?? ''),
                    name: (string) ($row['name'] ?? ''),
                    description: isset($row['description']) ? (string) $row['description'] : null,
                    externalId: isset($row['externalId']) ? (string) $row['externalId'] : null,
                ),
                $rows,
            );
            $result = $this->client->connections()->sync($appCode, $entries, $clientId, $removeUnlisted);

            return [
                'created' => $result->created,
                'updated' => $result->updated,
                'deleted' => $result->deleted,
            ];
        } catch (\Exception $e) {
            return [
                'created' => 0,
                'updated' => 0,
                'deleted' => 0,
                'error' => $e->getMessage(),
            ];
        }
    }

    /**
     * Sync one client-group's worth of subscriptions.
     *
     * @param array<array<string, mixed>> $subscriptions
     * @return array{created: int, updated: int, deleted: int, error?: string}
     */
    private function syncSubscriptionGroup(
        string $appCode,
        array $subscriptions,
        ?string $clientId,
        bool $removeUnlisted,
        ?string $targetBaseUrl,
    ): array {
        // Two sets contributing to the SAME (application, client) scope
        // (most commonly after syncGrouped() merges them) defining the same
        // subscription code is a configuration error — fail locally, naming
        // the code and scope, rather than sending a request the platform
        // will reject or silently keeping whichever row happened to be last.
        $duplicates = $this->findDuplicates(array_map(static fn($r) => (string) ($r['code'] ?? ''), $subscriptions));
        if ($duplicates !== []) {
            return [
                'created' => 0,
                'updated' => 0,
                'deleted' => 0,
                'error' => sprintf(
                    'Duplicate subscription code(s) for %s: %s',
                    $this->scopeLabel($appCode, $clientId),
                    implode(', ', $duplicates),
                ),
            ];
        }

        try {
            // Resolve every target BEFORE building the payload, and refuse
            // the whole sync if any is missing. The platform rejects an empty
            // target anyway, but with an opaque TARGET_REQUIRED for the batch;
            // naming the subscriptions here is what makes it fixable. Sending
            // only the resolvable ones is not an option: with removeUnlisted
            // the omitted subscriptions would be DELETED.
            $targets = [];
            $unresolved = [];
            foreach ($subscriptions as $i => $row) {
                // A row's own `_targetBaseUrl` (stamped by SyncDefinitionSet::
                // merge() from the set it originally came from) wins over
                // this group's base URL — set here from either the ORIGINAL
                // (unmerged) set's own `getTargetBaseUrl()` or the
                // synchronizer's configured default. Absent on a row that
                // never went through a merge, so single-set behaviour is
                // unchanged.
                $rowBaseUrl = (isset($row['_targetBaseUrl']) && is_string($row['_targetBaseUrl']) && $row['_targetBaseUrl'] !== '')
                    ? $row['_targetBaseUrl']
                    : $targetBaseUrl;
                $target = $this->resolveSubscriptionTarget($row, $rowBaseUrl);
                if ($target === null) {
                    $unresolved[] = (string) ($row['code'] ?? "#{$i}");
                    continue;
                }
                $targets[$i] = $target;
            }
            if ($unresolved !== []) {
                throw new \InvalidArgumentException(
                    'No delivery target for subscription(s): ' . implode(', ', $unresolved)
                    . '. `target` must be an absolute URL, or a path — which needs'
                    . ' flowcatalyst.subscriptions.target_base_url (or the set\'s'
                    . ' forClient() override) or app.url to resolve against.'
                );
            }

            $entries = array_map(
                function (array $row, int|string $i) use ($targets) {
                    $rawBindings = $row['eventTypes'] ?? (
                        isset($row['eventTypeCode']) ? [['eventTypeCode' => $row['eventTypeCode']]] : []
                    );
                    $bindings = array_map(
                        fn(array $b) => new EventTypeBinding(
                            eventTypeCode: (string) $b['eventTypeCode'],
                            filter: isset($b['filter']) ? (string) $b['filter'] : null,
                        ),
                        $rawBindings,
                    );
                    return new SyncSubscriptionEntry(
                        code: (string) ($row['code'] ?? ''),
                        name: (string) ($row['name'] ?? ''),
                        target: $targets[$i],
                        eventTypes: $bindings,
                        description: isset($row['description']) ? (string) $row['description'] : null,
                        connectionId: isset($row['connectionId']) ? (string) $row['connectionId'] : null,
                        connectionCode: isset($row['connectionCode']) ? (string) $row['connectionCode'] : null,
                        dispatchPoolCode: isset($row['dispatchPoolCode']) ? (string) $row['dispatchPoolCode'] : null,
                        mode: $row['mode'] ?? null,
                        maxRetries: isset($row['maxRetries']) ? (int) $row['maxRetries'] : null,
                        timeoutSeconds: isset($row['timeoutSeconds']) ? (int) $row['timeoutSeconds'] : null,
                        dataOnly: (bool) ($row['dataOnly'] ?? false),
                        sharedConnection: (bool) ($row['sharedConnection'] ?? false),
                    );
                },
                $subscriptions,
                array_keys($subscriptions),
            );
            $result = $this->client->subscriptions()->sync($appCode, $entries, $removeUnlisted, $clientId);

            return [
                'created' => $result->created,
                'updated' => $result->updated,
                'deleted' => $result->deleted,
            ];
        } catch (\Exception $e) {
            return [
                'created' => 0,
                'updated' => 0,
                'deleted' => 0,
                'error' => $e->getMessage(),
            ];
        }
    }

    /**
     * Sync dispatch pools for an application.
     *
     * @param string $appCode Application code
     * @param array<array<string, mixed>> $dispatchPools Dispatch pool definitions
     * @param bool $removeUnlisted Remove dispatch pools not in the local set
     * @return array{created: int, updated: int, deleted: int, error?: string}
     */
    private function syncDispatchPools(string $appCode, array $dispatchPools, bool $removeUnlisted): array
    {
        // Validate dispatch pool codes before syncing
        $validationErrors = $this->validateDispatchPools($dispatchPools);

        $duplicates = $this->findDuplicates(array_map(static fn($p) => (string) ($p['code'] ?? ''), $dispatchPools));
        if ($duplicates !== []) {
            $validationErrors[] = sprintf(
                'Duplicate dispatch pool code(s) for %s: %s',
                $this->scopeLabel($appCode, null),
                implode(', ', $duplicates),
            );
        }

        if (!empty($validationErrors)) {
            return [
                'created' => 0,
                'updated' => 0,
                'deleted' => 0,
                'error' => implode('; ', $validationErrors),
            ];
        }

        try {
            $entries = array_map(
                fn(array $row) => new SyncDispatchPoolEntry(
                    code: (string) ($row['code'] ?? ''),
                    name: (string) ($row['name'] ?? $row['code'] ?? ''),
                    description: isset($row['description']) ? (string) $row['description'] : null,
                    rateLimit: isset($row['rateLimit']) ? (int) $row['rateLimit'] : null,
                    concurrency: isset($row['concurrency']) ? (int) $row['concurrency'] : null,
                ),
                $dispatchPools,
            );
            $result = $this->client->dispatchPools()->sync($appCode, $entries, $removeUnlisted);

            return [
                'created' => $result->created,
                'updated' => $result->updated,
                'deleted' => $result->deleted,
            ];
        } catch (\Exception $e) {
            return [
                'created' => 0,
                'updated' => 0,
                'deleted' => 0,
                'error' => $e->getMessage(),
            ];
        }
    }

    /**
     * Validate dispatch pool definitions before syncing.
     *
     * @param array<array<string, mixed>> $dispatchPools Dispatch pool definitions
     * @return string[] Validation error messages
     */
    private function validateDispatchPools(array $dispatchPools): array
    {
        $errors = [];

        foreach ($dispatchPools as $pool) {
            $code = $pool['code'] ?? '';
            $error = DispatchPoolDefinition::validateCode($code);
            if ($error !== null) {
                $errors[] = $error;
            }
        }

        return $errors;
    }

    /**
     * Sync processes (workflow documentation) for an application.
     *
     * Accepts entries shaped like `ProcessDefinition::toArray()` (which
     * carry the full `code`) or scanner output from `#[AsProcess]` (which
     * carries `subdomain` + `processName` and relies on `$appCode` for
     * the first segment).
     *
     * @param string $appCode Application code
     * @param array<array<string, mixed>> $processes Process definitions
     * @param bool $removeUnlisted Archive CODE/API-sourced processes not in the local set
     * @return array{created: int, updated: int, deleted: int, error?: string}
     */
    private function syncProcesses(string $appCode, array $processes, bool $removeUnlisted): array
    {
        $codes = array_map(fn(array $row) => $this->processCode($row, $appCode), $processes);
        $duplicates = $this->findDuplicates($codes);
        if ($duplicates !== []) {
            return [
                'created' => 0,
                'updated' => 0,
                'deleted' => 0,
                'error' => sprintf(
                    'Duplicate process code(s) for %s: %s',
                    $this->scopeLabel($appCode, null),
                    implode(', ', $duplicates),
                ),
            ];
        }

        try {
            $entries = array_map(
                function (array $row, string $code) {
                    /** @var string[] $tags */
                    $tags = $row['tags'] ?? [];
                    return new SyncProcessEntry(
                        code: $code,
                        name: (string) ($row['name'] ?? ''),
                        body: (string) ($row['body'] ?? ''),
                        description: isset($row['description']) ? (string) $row['description'] : null,
                        diagramType: (string) ($row['diagramType'] ?? 'mermaid'),
                        tags: array_map(fn($t) => (string) $t, $tags),
                    );
                },
                $processes,
                $codes,
            );
            $result = $this->client->processes()->sync($appCode, $entries, $removeUnlisted);

            return [
                'created' => $result->created,
                'updated' => $result->updated,
                'deleted' => $result->deleted,
            ];
        } catch (\Exception $e) {
            return [
                'created' => 0,
                'updated' => 0,
                'deleted' => 0,
                'error' => $e->getMessage(),
            ];
        }
    }

    /**
     * Resolve the full process code from a definition row — the same
     * fallback `ProcessDefinition::toArray()`-vs-scanner-output shape
     * `syncProcesses()` and its dry-run/display counterparts already handle,
     * pulled out so the code can be computed once (for the duplicate check)
     * and reused for the actual entry.
     *
     * @param array<string, mixed> $row
     */
    private function processCode(array $row, string $appCode): string
    {
        return isset($row['code']) && $row['code'] !== ''
            ? (string) $row['code']
            : sprintf(
                '%s:%s:%s',
                $appCode,
                (string) ($row['subdomain'] ?? ''),
                (string) ($row['processName'] ?? ''),
            );
    }

    /**
     * Sync principals (users with roles) for an application.
     *
     * @param string $appCode Application code
     * @param array<array<string, mixed>> $principals Principal definitions
     * @param bool $removeUnlisted Remove SDK-synced roles for unlisted principals
     * @return array{created: int, updated: int, deleted: int, error?: string}
     */
    private function syncPrincipals(string $appCode, array $principals, bool $removeUnlisted): array
    {
        try {
            $entries = array_map(
                fn(array $row) => new SyncPrincipalEntry(
                    email: (string) ($row['email'] ?? ''),
                    name: (string) ($row['name'] ?? ''),
                    roles: $row['roles'] ?? [],
                    active: isset($row['active']) ? (bool) $row['active'] : null,
                    passwordHash: isset($row['passwordHash']) ? (string) $row['passwordHash'] : null,
                ),
                $principals,
            );
            $result = $this->client->principals()->sync($appCode, $entries, $removeUnlisted);

            return [
                'created' => $result->created,
                'updated' => $result->updated,
                'deleted' => $result->deleted,
            ];
        } catch (\Exception $e) {
            return [
                'created' => 0,
                'updated' => 0,
                'deleted' => 0,
                'error' => $e->getMessage(),
            ];
        }
    }

    /**
     * Sync scheduled jobs for an application.
     *
     * The platform's scheduled-jobs sync endpoint uses `archiveUnlisted` in
     * the body rather than `removeUnlisted` as a query string; we translate.
     *
     * Jobs are grouped by their resolved `clientId` (batch key, not per-job
     * on the wire) since the sync endpoint accepts one `clientId` for the
     * whole call — one `sync()` request is issued per distinct group. In
     * the common case (all jobs share one client, or none set one at all)
     * this is a single request, same as before.
     *
     * @param string $appCode Application code
     * @param array<array<string, mixed>> $jobs Scheduled-job definitions
     * @param bool $removeUnlisted Archive jobs present on the platform but missing locally
     * @return array{created: int, updated: int, deleted: int, error?: string}
     */
    private function syncScheduledJobs(string $appCode, array $jobs, bool $removeUnlisted): array
    {
        $groups = [];
        foreach ($jobs as $row) {
            $clientId = (is_array($row) && isset($row['clientId']) && is_string($row['clientId']) && $row['clientId'] !== '')
                ? $row['clientId']
                : '';
            $groups[$clientId][] = $row;
        }

        $created = 0;
        $updated = 0;
        $deleted = 0;
        $errors = [];

        foreach ($groups as $clientId => $groupJobs) {
            // Two sets contributing jobs to the SAME (application, clientId)
            // scope (most commonly after syncGrouped() merges them) defining
            // the same job code is a configuration error — fail locally
            // rather than sending a request the platform will reject or
            // silently keeping whichever row happened to be last.
            $duplicates = $this->findDuplicates(array_map(static fn($r) => (string) ($r['code'] ?? ''), $groupJobs));
            if ($duplicates !== []) {
                $errors[] = sprintf(
                    'Duplicate scheduled job code(s) for %s: %s',
                    $this->scopeLabel($appCode, $clientId !== '' ? $clientId : null),
                    implode(', ', $duplicates),
                );
                continue;
            }

            try {
                $entries = array_map(
                    fn(array $row) => new SyncScheduledJobEntry(
                        code: (string) ($row['code'] ?? ''),
                        name: (string) ($row['name'] ?? ''),
                        crons: array_map(static fn($c) => (string) $c, (array) ($row['crons'] ?? [])),
                        description: isset($row['description']) ? (string) $row['description'] : null,
                        timezone: isset($row['timezone']) ? (string) $row['timezone'] : 'UTC',
                        payload: $row['payload'] ?? null,
                        concurrent: (bool) ($row['concurrent'] ?? false),
                        tracksCompletion: (bool) ($row['tracksCompletion'] ?? false),
                        timeoutSeconds: isset($row['timeoutSeconds']) ? (int) $row['timeoutSeconds'] : null,
                        deliveryMaxAttempts: isset($row['deliveryMaxAttempts']) ? (int) $row['deliveryMaxAttempts'] : 3,
                        targetUrl: isset($row['targetUrl'])
                            ? (string) $row['targetUrl']
                            : $this->defaultScheduledJobTargetUrl,
                    ),
                    $groupJobs,
                );
                $result = $this->client->scheduledJobs()->sync(
                    applicationCode: $appCode,
                    jobs: $entries,
                    clientId: $clientId !== '' ? $clientId : null,
                    archiveUnlisted: $removeUnlisted,
                );

                $created += count($result['created'] ?? []);
                $updated += count($result['updated'] ?? []);
                $deleted += count($result['archived'] ?? []);
            } catch (\Exception $e) {
                $errors[] = $e->getMessage();
            }
        }

        $out = ['created' => $created, 'updated' => $updated, 'deleted' => $deleted];
        if ($errors !== []) {
            $out['error'] = implode('; ', $errors);
        }
        return $out;
    }

    /**
     * Publish a single OpenAPI document for an application. The platform
     * short-circuits on `unchanged` and archives the prior version when it
     * differs; we normalise the response into the standard
     * `{created, updated, deleted}` shape (plus `version` for visibility).
     *
     * @param string $appCode Application code
     * @param mixed $spec Parsed OpenAPI document (associative array or stdClass)
     * @return array{created: int, updated: int, deleted: int, error?: string, version?: string}
     */
    private function syncOpenapi(string $appCode, mixed $spec): array
    {
        try {
            $response = $this->client->request('POST', "/api/applications/{$appCode}/openapi/sync", [
                'json' => ['spec' => $spec],
            ]);

            $unchanged = (bool) ($response['unchanged'] ?? false);
            $archivedPriorVersion = $response['archivedPriorVersion'] ?? null;
            $version = isset($response['version']) ? (string) $response['version'] : '';

            $created = ($unchanged || $archivedPriorVersion !== null) ? 0 : 1;
            $updated = $archivedPriorVersion !== null ? 1 : 0;

            return [
                'created' => $created,
                'updated' => $updated,
                'deleted' => 0,
                'version' => $version,
            ];
        } catch (\Exception $e) {
            return [
                'created' => 0,
                'updated' => 0,
                'deleted' => 0,
                'error' => $e->getMessage(),
            ];
        }
    }
}
