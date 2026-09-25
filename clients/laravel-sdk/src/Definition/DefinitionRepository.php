<?php

declare(strict_types=1);

namespace FlowCatalyst\Definition;

use Illuminate\Support\Facades\File;

/**
 * Repository for storing and retrieving FlowCatalyst definitions.
 *
 * Definitions are cached to JSON files in storage for lazy loading.
 */
class DefinitionRepository
{
    private ?ScannedDefinitions $cached = null;

    public function __construct(
        private readonly string $cachePath,
        private readonly DefinitionScanner $scanner,
    ) {}

    /**
     * Get all definitions, loading from cache if available.
     */
    public function all(): ScannedDefinitions
    {
        if ($this->cached !== null) {
            return $this->cached;
        }

        if ($this->cacheExists()) {
            $this->cached = $this->loadFromCache();
            return $this->cached;
        }

        // Return empty if no cache exists
        return new ScannedDefinitions();
    }

    /**
     * Get all role definitions.
     *
     * @return array<array<string, mixed>>
     */
    public function roles(): array
    {
        return $this->all()->roles;
    }

    /**
     * Get all standalone permission definitions.
     *
     * @return array<array<string, mixed>>
     */
    public function permissions(): array
    {
        return $this->all()->permissions;
    }

    /**
     * Get all event type definitions.
     *
     * @return array<array<string, mixed>>
     */
    public function eventTypes(): array
    {
        return $this->all()->eventTypes;
    }

    /**
     * Get all subscription definitions.
     *
     * @return array<array<string, mixed>>
     */
    public function subscriptions(): array
    {
        return $this->all()->subscriptions;
    }

    /**
     * Get all connection definitions.
     *
     * @return array<array<string, mixed>>
     */
    public function connections(): array
    {
        return $this->all()->connections;
    }

    /**
     * Get all dispatch pool definitions.
     *
     * @return array<array<string, mixed>>
     */
    public function dispatchPools(): array
    {
        return $this->all()->dispatchPools;
    }

    /**
     * Get all process (workflow documentation) definitions.
     *
     * @return array<array<string, mixed>>
     */
    public function processes(): array
    {
        return $this->all()->processes;
    }

    /**
     * Get all scheduled-job definitions.
     *
     * @return array<array<string, mixed>>
     */
    public function scheduledJobs(): array
    {
        return $this->all()->scheduledJobs;
    }

    /**
     * Scan and cache definitions from the given paths.
     *
     * @param string[] $paths Directories to scan
     */
    public function scanAndCache(array $paths): ScannedDefinitions
    {
        $definitions = $this->scanner->scan($paths);
        $this->saveToCache($definitions);
        $this->cached = $definitions;

        return $definitions;
    }

    /**
     * Clear the cached definitions.
     */
    public function clearCache(): void
    {
        $this->cached = null;

        if ($this->cacheExists()) {
            File::delete($this->getCacheFilePath());
        }
    }

    /**
     * Check if the cache exists.
     */
    public function cacheExists(): bool
    {
        return File::exists($this->getCacheFilePath());
    }

    /**
     * Get the cache file path.
     */
    public function getCacheFilePath(): string
    {
        return $this->cachePath . '/definitions.json';
    }

    /**
     * Load definitions from cache file.
     */
    private function loadFromCache(): ScannedDefinitions
    {
        $content = File::get($this->getCacheFilePath());
        $data = json_decode($content, true);

        if (!is_array($data)) {
            return new ScannedDefinitions();
        }

        return ScannedDefinitions::fromArray($data);
    }

    /**
     * Save definitions to cache file.
     */
    private function saveToCache(ScannedDefinitions $definitions): void
    {
        File::ensureDirectoryExists($this->cachePath);
        File::put(
            $this->getCacheFilePath(),
            json_encode($definitions->toArray(), JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES)
        );
    }
}
