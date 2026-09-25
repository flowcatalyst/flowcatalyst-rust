<?php

declare(strict_types=1);

namespace FlowCatalyst\Definition;

use FlowCatalyst\Attributes\AsConnection;
use FlowCatalyst\Attributes\AsDispatchPool;
use FlowCatalyst\Attributes\AsEventType;
use FlowCatalyst\Attributes\AsPermission;
use FlowCatalyst\Attributes\AsProcess;
use FlowCatalyst\Attributes\AsRole;
use FlowCatalyst\Attributes\AsScheduledJob;
use FlowCatalyst\Attributes\AsSubscription;
use Illuminate\Support\Facades\File;
use ReflectionClass;
use Symfony\Component\Finder\Finder;

/**
 * Scans directories for FlowCatalyst attribute definitions.
 */
class DefinitionScanner
{
    /**
     * Scan configured paths for FlowCatalyst definitions.
     *
     * @param string[] $paths Directories to scan
     * @return ScannedDefinitions
     */
    public function scan(array $paths, ?string $applicationCode = null): ScannedDefinitions
    {
        // App code used to resolve #[AsPermission] segments + role permission
        // class-refs. Falls back to the configured application code.
        $applicationCode ??= (function () {
            $code = function_exists('config') ? config('flowcatalyst.application_code') : null;
            return is_string($code) && $code !== '' ? $code : null;
        })();

        $roles = [];
        $permissions = [];
        $eventTypes = [];
        $subscriptions = [];
        $connections = [];
        $dispatchPools = [];
        $processes = [];
        $scheduledJobs = [];

        foreach ($paths as $path) {
            if (!File::isDirectory($path)) {
                continue;
            }

            $finder = new Finder();
            $finder->files()->in($path)->name('*.php');

            foreach ($finder as $file) {
                $classes = $this->getClassesFromFile($file->getRealPath());

                foreach ($classes as $className) {
                    $this->processClass($className, $applicationCode, $roles, $permissions, $eventTypes, $subscriptions, $connections, $dispatchPools, $processes, $scheduledJobs);
                }
            }
        }

        return new ScannedDefinitions(
            roles: $roles,
            permissions: $permissions,
            eventTypes: $eventTypes,
            subscriptions: $subscriptions,
            connections: $connections,
            dispatchPools: $dispatchPools,
            processes: $processes,
            scheduledJobs: $scheduledJobs,
        );
    }

    /**
     * Extract class names from a PHP file.
     *
     * @return string[]
     */
    private function getClassesFromFile(string $filepath): array
    {
        $content = file_get_contents($filepath);
        if ($content === false) {
            return [];
        }

        $classes = [];
        $namespace = '';

        // Extract namespace
        if (preg_match('/namespace\s+([^;]+);/', $content, $matches)) {
            $namespace = $matches[1];
        }

        // Extract class names. Allow leading class modifiers — `final`,
        // `abstract`, `readonly` (in any combination) — so a definition
        // declared `final class CommentCreated` is discovered, not just a bare
        // `class Foo`. Without this, final/abstract definition classes are
        // silently skipped.
        if (preg_match_all('/^\s*(?:(?:final|abstract|readonly)\s+)*class\s+(\w+)/mi', $content, $matches)) {
            foreach ($matches[1] as $className) {
                $fullClassName = $namespace ? "{$namespace}\\{$className}" : $className;
                if (class_exists($fullClassName)) {
                    $classes[] = $fullClassName;
                }
            }
        }

        return $classes;
    }

    /**
     * Process a class for FlowCatalyst attributes.
     *
     * @param array<array<string, mixed>> $roles
     * @param array<array<string, mixed>> $eventTypes
     * @param array<array<string, mixed>> $subscriptions
     * @param array<array<string, mixed>> $connections
     * @param array<array<string, mixed>> $dispatchPools
     * @param array<array<string, mixed>> $processes
     * @param array<array<string, mixed>> $scheduledJobs
     */
    private function processClass(
        string $className,
        ?string $applicationCode,
        array &$roles,
        array &$permissions,
        array &$eventTypes,
        array &$subscriptions,
        array &$connections,
        array &$dispatchPools,
        array &$processes,
        array &$scheduledJobs
    ): void {
        try {
            $reflection = new ReflectionClass($className);
        } catch (\ReflectionException) {
            return;
        }

        // Check for AsPermission attribute (standalone permission definitions).
        $permissionAttributes = $reflection->getAttributes(AsPermission::class);
        foreach ($permissionAttributes as $attribute) {
            /** @var AsPermission $instance */
            $instance = $attribute->newInstance();
            $app = $this->resolveApplication($className, $instance);
            // Permissions/roles bake the app code into their array (it forms part
            // of the code), so feed the resolved app — falling back to the scan
            // default — instead of always the default.
            $permissions[] = array_merge($instance->toArray($app ?? $applicationCode), [
                '_class' => $className,
                '_application' => $app,
            ]);
        }

        // Check for AsRole attribute
        $roleAttributes = $reflection->getAttributes(AsRole::class);
        foreach ($roleAttributes as $attribute) {
            /** @var AsRole $instance */
            $instance = $attribute->newInstance();
            $app = $this->resolveApplication($className, $instance);
            $roles[] = array_merge($instance->toArray($app ?? $applicationCode), [
                '_class' => $className,
                '_application' => $app,
            ]);
        }

        // Check for AsEventType attribute
        $eventTypeAttributes = $reflection->getAttributes(AsEventType::class);
        foreach ($eventTypeAttributes as $attribute) {
            /** @var AsEventType $instance */
            $instance = $attribute->newInstance();
            $eventTypes[] = array_merge($instance->toArray(), [
                '_class' => $className,
                '_application' => $this->resolveApplication($className, $instance),
            ]);
        }

        // Check for AsSubscription attribute
        $subscriptionAttributes = $reflection->getAttributes(AsSubscription::class);
        foreach ($subscriptionAttributes as $attribute) {
            /** @var AsSubscription $instance */
            $instance = $attribute->newInstance();
            $subscriptions[] = array_merge($instance->toArray(), [
                '_class' => $className,
                '_application' => $this->resolveApplication($className, $instance),
                'client' => $this->resolveClient($instance),
            ]);
        }

        // Check for AsConnection attribute
        $connectionAttributes = $reflection->getAttributes(AsConnection::class);
        foreach ($connectionAttributes as $attribute) {
            /** @var AsConnection $instance */
            $instance = $attribute->newInstance();
            $connections[] = array_merge($instance->toArray(), [
                '_class' => $className,
                '_application' => $this->resolveApplication($className, $instance),
                'client' => $this->resolveClient($instance),
            ]);
        }

        // Check for AsDispatchPool attribute
        $dispatchPoolAttributes = $reflection->getAttributes(AsDispatchPool::class);
        foreach ($dispatchPoolAttributes as $attribute) {
            /** @var AsDispatchPool $instance */
            $instance = $attribute->newInstance();
            $dispatchPools[] = array_merge($instance->toArray(), [
                '_class' => $className,
                '_application' => $this->resolveApplication($className, $instance),
            ]);
        }

        // Check for AsProcess attribute
        $processAttributes = $reflection->getAttributes(AsProcess::class);
        foreach ($processAttributes as $attribute) {
            /** @var AsProcess $instance */
            $instance = $attribute->newInstance();
            $processes[] = array_merge($instance->toArray(), [
                '_class' => $className,
                '_application' => $this->resolveApplication($className, $instance),
            ]);
        }

        // Check for AsScheduledJob attribute
        $scheduledJobAttributes = $reflection->getAttributes(AsScheduledJob::class);
        foreach ($scheduledJobAttributes as $attribute) {
            /** @var AsScheduledJob $instance */
            $instance = $attribute->newInstance();
            $scheduledJobs[] = array_merge($instance->toArray(), [
                '_class' => $className,
                '_application' => $this->resolveApplication($className, $instance),
            ]);
        }
    }

    /**
     * Resolve which application code a definition belongs to, for codebases
     * that define definitions for more than one application. Order:
     *   1. explicit `application:` on the attribute (where supported),
     *   2. longest-prefix match in `flowcatalyst.definitions.application_map`,
     *   3. null — the sync command falls back to the global default / --app.
     */
    private function resolveApplication(string $className, object $instance): ?string
    {
        $explicit = (property_exists($instance, 'application') && is_string($instance->application) && $instance->application !== '')
            ? $instance->application
            : null;

        return $explicit ?? $this->matchApplicationMap($className);
    }

    /**
     * Resolve which FlowCatalyst client a subscription/connection definition
     * belongs to (single-tenant apps — see `flowcatalyst.client`). Order:
     *   1. explicit `client:` on the attribute,
     *   2. the config default `flowcatalyst.client` (`FLOWCATALYST_CLIENT`),
     *   3. null — global.
     * Unlike `resolveApplication`, there is no namespace map: a codebase that
     * defines definitions for more than one client builds one
     * `SyncDefinitionSet` per client instead (`SyncDefinitionSet::forClient()`).
     */
    private function resolveClient(object $instance): ?string
    {
        $explicit = (property_exists($instance, 'client') && is_string($instance->client) && $instance->client !== '')
            ? $instance->client
            : null;
        if ($explicit !== null) {
            return $explicit;
        }

        $default = function_exists('config') ? config('flowcatalyst.client') : null;
        return is_string($default) && $default !== '' ? $default : null;
    }

    /**
     * Longest-prefix match of a class's fully-qualified name against the
     * configured namespace → application-code map. Lets a whole package/module
     * inherit one application code (the consumer owns the mapping).
     */
    private function matchApplicationMap(string $className): ?string
    {
        $map = function_exists('config') ? config('flowcatalyst.definitions.application_map', []) : [];
        if (!is_array($map)) {
            return null;
        }

        $needle = ltrim($className, '\\');
        $bestApp = null;
        $bestLen = -1;
        foreach ($map as $prefix => $app) {
            $prefix = ltrim((string) $prefix, '\\');
            if ($prefix !== '' && is_string($app) && $app !== ''
                && str_starts_with($needle, $prefix) && strlen($prefix) > $bestLen) {
                $bestApp = $app;
                $bestLen = strlen($prefix);
            }
        }

        return $bestApp;
    }
}
