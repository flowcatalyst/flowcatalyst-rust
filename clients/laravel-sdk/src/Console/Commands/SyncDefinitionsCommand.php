<?php

declare(strict_types=1);

namespace FlowCatalyst\Console\Commands;

use FlowCatalyst\Definition\DefinitionRepository;
use FlowCatalyst\Sync\DefinitionSynchronizer;
use FlowCatalyst\Sync\ProvidesSyncDefinitionSets;
use FlowCatalyst\Sync\SyncDefinitionSet;
use FlowCatalyst\Sync\SyncOptions;
use FlowCatalyst\Sync\SyncResult;
use Illuminate\Console\Command;

/**
 * Sync FlowCatalyst definitions to the platform.
 *
 * This command reads from the cached definitions (created by flowcatalyst:scan)
 * and syncs them to the platform. For programmatic sync without attributes,
 * use the DefinitionSynchronizer service directly.
 */
class SyncDefinitionsCommand extends Command
{
    protected $signature = 'flowcatalyst:sync
                            {--app= : Application code to sync (defaults to config value)}
                            {--roles : Sync only roles}
                            {--event-types : Sync only event types}
                            {--subscriptions : Sync only subscriptions}
                            {--connections : Sync only connections}
                            {--dispatch-pools : Sync only dispatch pools}
                            {--principals : Sync only principals}
                            {--processes : Sync only processes}
                            {--scheduled-jobs : Sync only scheduled jobs}
                            {--openapi : Publish only the attached OpenAPI document}
                            {--openapi-file= : Path to a JSON/YAML OpenAPI file to attach before sync}
                            {--remove-unlisted : Remove definitions not in local cache}
                            {--no-spatie : Skip seeding roles/permissions into the local Spatie tables}
                            {--dry-run : Show what would be synced without actually syncing}';

    protected $description = 'Sync FlowCatalyst definitions to the platform';

    public function handle(
        DefinitionRepository $repository,
        DefinitionSynchronizer $synchronizer
    ): int {
        if (!$repository->cacheExists()) {
            $this->error('No cached definitions found. Run `php artisan flowcatalyst:scan` first.');
            return Command::FAILURE;
        }

        // The DEFAULT application code, used for definitions that don't resolve
        // their own (via the attribute `application:` or the
        // `definitions.application_map`). May be empty when every definition
        // carries its own app — groupByApplication then flags any that can't be
        // placed.
        $appCode = $this->option('app') ?: config('flowcatalyst.application_code');

        $dryRun = $this->option('dry-run');
        $removeUnlisted = $this->option('remove-unlisted');

        if ($dryRun) {
            $this->warn('DRY RUN - No changes will be made');
            $this->newLine();
        }

        // Build options from command flags
        $syncAll = !$this->option('roles')
            && !$this->option('event-types')
            && !$this->option('subscriptions')
            && !$this->option('connections')
            && !$this->option('dispatch-pools')
            && !$this->option('principals')
            && !$this->option('processes')
            && !$this->option('scheduled-jobs')
            && !$this->option('openapi');
        $options = new SyncOptions(
            removeUnlisted: $removeUnlisted,
            syncRoles: $syncAll || $this->option('roles'),
            syncEventTypes: $syncAll || $this->option('event-types'),
            syncSubscriptions: $syncAll || $this->option('subscriptions'),
            syncConnections: $syncAll || $this->option('connections'),
            syncDispatchPools: $syncAll || $this->option('dispatch-pools'),
            syncPrincipals: $syncAll || $this->option('principals'),
            syncProcesses: $syncAll || $this->option('processes'),
            syncScheduledJobs: $syncAll || $this->option('scheduled-jobs'),
            syncOpenapi: $syncAll || $this->option('openapi'),
        );

        // Partition cached definitions into per-application buckets, so one
        // codebase can target multiple applications (each synced to its own
        // app via the application-scoped endpoints).
        $scannedData = $repository->all()->toArray();
        $groups = $this->groupByApplication($scannedData, $appCode);

        // Definitions that resolve no application AND have no default can't be
        // addressed to an application-scoped endpoint. Report and skip them,
        // then sync whatever remains — an unplaceable definition must not block
        // every other application in the same codebase.
        if (isset($groups[''])) {
            $this->reportUnplaceable($groups['']);
            unset($groups['']);
        }

        // OpenAPI document (when an explicit file is given) is application-level;
        // it attaches to the default app's group only.
        $openapiSpec = $this->loadOpenapiSpec();
        if ($openapiSpec === false) {
            return Command::FAILURE;
        }

        // One (global) set per scanned application...
        $sets = [];
        foreach ($groups as $groupApp => $data) {
            $set = SyncDefinitionSet::fromScannedDefinitions($groupApp, $data);
            if ($openapiSpec !== null && $groupApp === $appCode) {
                $set = $set->withOpenapiSpec($openapiSpec);
            }
            if (!$set->isEmpty()) {
                $sets[] = $set;
            }
        }

        // ...plus every set yielded by a configured provider — the
        // multi-tenant path, where the tenant list is runtime data no
        // attribute could express (global + per-client sets, built in code).
        $sets = [...$sets, ...$this->resolveProviderSets()];

        if ($sets === []) {
            $this->info('No definitions to sync.');
            return Command::SUCCESS;
        }

        $appsInvolved = array_values(array_unique(array_map(
            static fn(SyncDefinitionSet $s) => $s->applicationCode,
            $sets,
        )));
        if (count($appsInvolved) > 1) {
            $this->info(sprintf('Syncing %d applications: %s', count($appsInvolved), implode(', ', $appsInvolved)));
            $this->newLine();
        }

        if ($dryRun) {
            foreach ($sets as $set) {
                $label = $set->isGlobal() ? $set->applicationCode : "{$set->applicationCode} (client: {$set->getClient()})";
                $this->info("Would sync to application: {$label}");
                $this->showDryRunOutput($set, $options);
                $this->displayPermissionSummary($set);
            }
            return Command::SUCCESS;
        }

        // Grouped by application, global set(s) before client set(s) —
        // a client-scoped subscription may reference a global connection —
        // with results for sets sharing an application code summed together.
        $results = $synchronizer->syncGrouped($sets, $options);

        $hadErrors = false;
        foreach ($results as $groupApp => $result) {
            $this->info("Synced definitions for application: {$groupApp}");
            $this->displayResults($result);
            if ($result->hasErrors()) {
                $hadErrors = true;
            }
        }

        // Mirror roles + permissions into the local Spatie tables (so the
        // app's authorization model matches what it just pushed). FlowCatalyst
        // has no standalone permission entity, so permissions reach the
        // platform via the roles above; this keeps the LOCAL side in step.
        foreach ($sets as $set) {
            $this->displayPermissionSummary($set);
            $this->seedSpatie($set);
        }

        return $hadErrors ? Command::FAILURE : Command::SUCCESS;
    }

    /**
     * Partition scanned definitions into per-application buckets keyed by the
     * resolved application code (each definition's `_application`, else the
     * default). Definitions with neither land in the '' bucket (an error).
     *
     * @param array<string, mixed> $scannedData
     * @param string|null $defaultAppCode
     * @return array<string, array<string, array<int, array<string, mixed>>>>
     */
    private function groupByApplication(array $scannedData, ?string $defaultAppCode): array
    {
        $categories = ['roles', 'permissions', 'eventTypes', 'subscriptions', 'connections', 'dispatchPools', 'principals', 'processes', 'scheduledJobs'];
        $default = is_string($defaultAppCode) ? $defaultAppCode : '';

        $groups = [];
        foreach ($categories as $category) {
            $defs = $scannedData[$category] ?? [];
            if (!is_array($defs)) {
                continue;
            }
            foreach ($defs as $def) {
                $app = (is_array($def) && isset($def['_application']) && is_string($def['_application']) && $def['_application'] !== '')
                    ? $def['_application']
                    : $default;
                $groups[$app][$category][] = $def;
            }
        }

        return $groups;
    }

    /**
     * Report the definitions that resolved no application code — no
     * `application:` on the attribute, no `definitions.application_map` match,
     * and no default from --app / FLOWCATALYST_APP_CODE. They are skipped
     * rather than fatal: there is no endpoint to address them to, but every
     * other application still syncs.
     *
     * @param array<string, array<int, mixed>> $unplaceable
     */
    private function reportUnplaceable(array $unplaceable): void
    {
        $count = array_sum(array_map('count', $unplaceable));

        $this->warn(sprintf('Skipped %d definition(s) with no application code:', $count));
        foreach ($unplaceable as $category => $defs) {
            foreach ($defs as $def) {
                $this->line(sprintf('  - %s: %s', $category, $this->definitionLabel($def)));
            }
        }
        $this->line('  To sync these, set FLOWCATALYST_APP_CODE (or --app), map their namespace in');
        $this->line('  flowcatalyst.definitions.application_map, or set application: on the attribute.');
        $this->newLine();
    }

    /**
     * Best-effort label for a scanned definition — the declaring class, else
     * whatever identifier its category carries.
     */
    private function definitionLabel(mixed $def): string
    {
        if (is_string($def)) {
            return $def !== '' ? $def : '(unnamed)';
        }

        if (is_array($def)) {
            foreach (['_class', 'code', 'name', 'permission'] as $key) {
                $value = $def[$key] ?? null;
                if (is_string($value) && $value !== '') {
                    return $value;
                }
            }
        }

        return '(unnamed)';
    }

    /**
     * Read + decode the --openapi-file option. Returns the decoded spec array,
     * null when no file was given, or false on a (reported) error.
     *
     * @return array<string, mixed>|null|false
     */
    private function loadOpenapiSpec(): array|null|false
    {
        $openapiFile = $this->option('openapi-file');
        if (!is_string($openapiFile) || $openapiFile === '') {
            return null;
        }
        if (!is_file($openapiFile)) {
            $this->error("OpenAPI file not found: {$openapiFile}");
            return false;
        }
        $raw = file_get_contents($openapiFile);
        if ($raw === false) {
            $this->error("Failed to read OpenAPI file: {$openapiFile}");
            return false;
        }
        $decoded = json_decode($raw, true);
        if (!is_array($decoded)) {
            $this->error("OpenAPI file is not valid JSON: {$openapiFile}");
            return false;
        }
        return $decoded;
    }

    /**
     * Definition sets supplied by configured providers
     * (`flowcatalyst.definitions.set_providers`) — the multi-tenant path.
     * Each configured class is resolved from the container and must
     * implement {@see ProvidesSyncDefinitionSets}; an unresolvable or
     * non-conforming entry is reported and skipped rather than fatal, so one
     * bad provider doesn't block the scanned attribute definitions (or other
     * providers) from syncing.
     *
     * @return SyncDefinitionSet[]
     */
    private function resolveProviderSets(): array
    {
        $classes = config('flowcatalyst.definitions.set_providers', []);
        if (!is_array($classes)) {
            return [];
        }

        $sets = [];
        foreach ($classes as $class) {
            if (!is_string($class) || $class === '') {
                continue;
            }

            try {
                $provider = app($class);
            } catch (\Throwable $e) {
                $this->warn("Could not resolve FlowCatalyst set provider [{$class}]: {$e->getMessage()}");
                continue;
            }

            if (!$provider instanceof ProvidesSyncDefinitionSets) {
                $this->warn("FlowCatalyst set provider [{$class}] does not implement ProvidesSyncDefinitionSets — skipped.");
                continue;
            }

            foreach ($provider->syncDefinitionSets() as $set) {
                if ($set instanceof SyncDefinitionSet && !$set->isEmpty()) {
                    $sets[] = $set;
                }
            }
        }

        return $sets;
    }

    /**
     * Report how many distinct permissions were synced (they reach the platform
     * via the roles that grant them — FlowCatalyst has no standalone permission).
     */
    private function displayPermissionSummary(SyncDefinitionSet $definitions): void
    {
        $permissions = [];

        foreach ($definitions->getRoles() as $role) {
            foreach (($role['permissions'] ?? []) as $permission) {
                if (is_string($permission) && $permission !== '') {
                    $permissions[$permission] = true;
                }
            }
        }

        foreach ($definitions->getPermissions() as $permission) {
            $name = is_array($permission) ? ($permission['permission'] ?? null) : (string) $permission;
            if (is_string($name) && $name !== '') {
                $permissions[$name] = true;
            }
        }

        if ($permissions === []) {
            return;
        }

        $this->info(sprintf(
            'Permissions: %d distinct, granted to the platform via %d role(s).',
            count($permissions),
            count($definitions->getRoles()),
        ));
    }

    /**
     * Seed roles + permissions into the local Spatie tables.
     */
    private function seedSpatie(SyncDefinitionSet $definitions): void
    {
        if ($this->option('no-spatie') || !config('flowcatalyst.definitions.seed_spatie', true)) {
            return;
        }

        $guard = (string) config('flowcatalyst.oidc.roles_guard', 'web');
        $seeder = new \FlowCatalyst\Sync\SpatieSeeder($guard);

        if (!$seeder->isAvailable()) {
            return; // spatie/laravel-permission not installed — nothing to seed.
        }

        try {
            $counts = $seeder->seed($definitions);
            $this->newLine();
            $this->info(sprintf(
                'Seeded into Spatie [%s]: %d role(s), %d permission(s).',
                $guard,
                $counts['roles'],
                $counts['permissions'],
            ));
        } catch (\Throwable $e) {
            $this->newLine();
            $this->warn('Spatie seeding skipped: ' . $e->getMessage());
        }
    }

    /**
     * Show what would be synced without actually syncing.
     */
    private function showDryRunOutput(SyncDefinitionSet $definitions, SyncOptions $options): void
    {
        if ($options->syncRoles && $definitions->hasRoles()) {
            $this->info('Roles to sync:');
            foreach ($definitions->getRoles() as $role) {
                $perms = $role['permissions'] ?? [];
                $suffix = $perms === [] ? '' : '  (' . implode(', ', $perms) . ')';
                $this->line("  - {$role['name']}{$suffix}");
            }
            $this->newLine();
        }

        if ($definitions->hasPermissions()) {
            $this->info('Permissions (seeded locally; reach FlowCatalyst via the roles that grant them):');
            foreach ($definitions->getPermissions() as $perm) {
                $name = is_array($perm) ? ($perm['permission'] ?? '') : (string) $perm;
                $this->line("  - {$name}");
            }
            $this->newLine();
        }

        if ($options->syncEventTypes && $definitions->hasEventTypes()) {
            $this->info('Event types to sync:');
            foreach ($definitions->getEventTypes() as $et) {
                $this->line("  - {$et['code']}");
            }
            $this->newLine();
        }

        if ($options->syncConnections && $definitions->hasConnections()) {
            $this->info('Connections to sync:');
            foreach ($definitions->getConnections() as $conn) {
                $this->line("  - {$conn['code']}");
            }
            $this->newLine();
        }

        if ($options->syncSubscriptions && $definitions->hasSubscriptions()) {
            $this->info('Subscriptions to sync:');
            foreach ($definitions->getSubscriptions() as $sub) {
                $this->line("  - {$sub['code']}");
            }
            $this->newLine();
        }

        if ($options->syncDispatchPools && $definitions->hasDispatchPools()) {
            $this->info('Dispatch pools to sync:');
            foreach ($definitions->getDispatchPools() as $pool) {
                $this->line("  - {$pool['code']}");
            }
            $this->newLine();
        }

        if ($options->syncProcesses && $definitions->hasProcesses()) {
            $this->info('Processes to sync:');
            foreach ($definitions->getProcesses() as $process) {
                $code = $process['code']
                    ?? sprintf(
                        '%s:%s:%s',
                        $definitions->applicationCode,
                        $process['subdomain'] ?? '',
                        $process['processName'] ?? '',
                    );
                $this->line("  - {$code}");
            }
            $this->newLine();
        }

        if ($options->syncScheduledJobs && $definitions->hasScheduledJobs()) {
            $this->info('Scheduled jobs to sync:');
            foreach ($definitions->getScheduledJobs() as $job) {
                $crons = implode(' | ', (array) ($job['crons'] ?? []));
                $this->line("  - {$job['code']}  [{$crons}]");
            }
            $this->newLine();
        }

        if ($options->syncOpenapi && $definitions->hasOpenapiSpec()) {
            $this->info('OpenAPI document attached — will publish on sync.');
            $this->newLine();
        }
    }

    /**
     * Display sync results.
     */
    private function displayResults(SyncResult $result): void
    {
        // Show errors if any
        if ($result->hasErrors()) {
            $this->newLine();
            $this->error('Sync completed with errors:');
            foreach ($result->getErrors() as $type => $error) {
                $this->error("  {$type}: {$error}");
            }
        }

        // Summary table
        $this->newLine();
        $this->info('Sync Summary:');
        $this->table(
            ['Type', 'Created', 'Updated', 'Deleted'],
            [
                [
                    'Roles',
                    $result->roles['created'] ?? 0,
                    $result->roles['updated'] ?? 0,
                    $result->roles['deleted'] ?? 0,
                ],
                [
                    'Event Types',
                    $result->eventTypes['created'] ?? 0,
                    $result->eventTypes['updated'] ?? 0,
                    $result->eventTypes['deleted'] ?? 0,
                ],
                [
                    'Connections',
                    $result->connections['created'] ?? 0,
                    $result->connections['updated'] ?? 0,
                    $result->connections['deleted'] ?? 0,
                ],
                [
                    'Subscriptions',
                    $result->subscriptions['created'] ?? 0,
                    $result->subscriptions['updated'] ?? 0,
                    $result->subscriptions['deleted'] ?? 0,
                ],
                [
                    'Dispatch Pools',
                    $result->dispatchPools['created'] ?? 0,
                    $result->dispatchPools['updated'] ?? 0,
                    $result->dispatchPools['deleted'] ?? 0,
                ],
                [
                    'Processes',
                    $result->processes['created'] ?? 0,
                    $result->processes['updated'] ?? 0,
                    $result->processes['deleted'] ?? 0,
                ],
                [
                    'Principals',
                    $result->principals['created'] ?? 0,
                    $result->principals['updated'] ?? 0,
                    $result->principals['deleted'] ?? 0,
                ],
                [
                    'Scheduled Jobs',
                    $result->scheduledJobs['created'] ?? 0,
                    $result->scheduledJobs['updated'] ?? 0,
                    $result->scheduledJobs['deleted'] ?? 0,
                ],
                [
                    'OpenAPI'
                        . (isset($result->openapi['version']) && $result->openapi['version'] !== ''
                            ? ' (' . $result->openapi['version'] . ')'
                            : ''),
                    $result->openapi['created'] ?? 0,
                    $result->openapi['updated'] ?? 0,
                    $result->openapi['deleted'] ?? 0,
                ],
            ]
        );
    }
}
