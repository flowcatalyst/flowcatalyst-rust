<?php

declare(strict_types=1);

namespace FlowCatalyst\Console\Commands;

use FlowCatalyst\Definition\DefinitionRepository;
use FlowCatalyst\Sync\DispatchPoolDefinition;
use FlowCatalyst\Sync\RoleDefinition;
use Illuminate\Console\Command;

/**
 * Scan for FlowCatalyst definitions and cache them.
 */
class ScanDefinitionsCommand extends Command
{
    protected $signature = 'flowcatalyst:scan
                            {--clear : Clear the cache before scanning}';

    protected $description = 'Scan for FlowCatalyst definitions (roles, event types, subscriptions) and cache them';

    public function handle(DefinitionRepository $repository): int
    {
        if ($this->option('clear')) {
            $repository->clearCache();
            $this->info('Cache cleared.');
        }

        $paths = config('flowcatalyst.definitions.paths', [app_path('FlowCatalyst')]);

        $this->info('Scanning paths for FlowCatalyst definitions...');
        foreach ($paths as $path) {
            $this->line("  - {$path}");
        }

        $definitions = $repository->scanAndCache($paths);

        $this->newLine();
        $this->info('Scan complete!');
        $this->table(
            ['Type', 'Count'],
            [
                ['Roles', count($definitions->roles)],
                ['Permissions', count($definitions->permissions)],
                ['Event Types', count($definitions->eventTypes)],
                ['Subscriptions', count($definitions->subscriptions)],
                ['Connections', count($definitions->connections)],
                ['Dispatch Pools', count($definitions->dispatchPools)],
            ]
        );

        // Validate role names
        $hasErrors = false;
        foreach ($definitions->roles as $role) {
            $name = $role['name'] ?? '';
            $error = RoleDefinition::validateName($name);
            if ($error !== null) {
                $this->error("Invalid role: {$error}");
                $hasErrors = true;
            }
        }

        // Validate dispatch pool codes
        foreach ($definitions->dispatchPools as $pool) {
            $code = $pool['code'] ?? '';
            $error = DispatchPoolDefinition::validateCode($code);
            if ($error !== null) {
                $this->error("Invalid dispatch pool: {$error}");
                $hasErrors = true;
            }
        }

        if ($hasErrors) {
            $this->newLine();
            $this->error('Validation errors found. Please fix the above issues before syncing.');
            return Command::FAILURE;
        }

        if (!$definitions->isEmpty()) {
            $this->newLine();
            $this->info('Definitions cached to: ' . $repository->getCacheFilePath());
        }

        return Command::SUCCESS;
    }
}
