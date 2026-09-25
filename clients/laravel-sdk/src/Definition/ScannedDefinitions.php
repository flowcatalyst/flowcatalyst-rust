<?php

declare(strict_types=1);

namespace FlowCatalyst\Definition;

/**
 * Container for scanned FlowCatalyst definitions.
 */
final class ScannedDefinitions
{
    /**
     * @param array<array<string, mixed>> $roles
     * @param array<array<string, mixed>> $eventTypes
     * @param array<array<string, mixed>> $subscriptions
     * @param array<array<string, mixed>> $connections
     * @param array<array<string, mixed>> $dispatchPools
     * @param array<array<string, mixed>> $processes
     * @param array<array<string, mixed>> $scheduledJobs
     */
    public function __construct(
        public readonly array $roles = [],
        public readonly array $permissions = [],
        public readonly array $eventTypes = [],
        public readonly array $subscriptions = [],
        public readonly array $connections = [],
        public readonly array $dispatchPools = [],
        public readonly array $processes = [],
        public readonly array $scheduledJobs = [],
    ) {}

    /**
     * Check if there are any definitions.
     */
    public function isEmpty(): bool
    {
        return empty($this->roles)
            && empty($this->permissions)
            && empty($this->eventTypes)
            && empty($this->subscriptions)
            && empty($this->connections)
            && empty($this->dispatchPools)
            && empty($this->processes)
            && empty($this->scheduledJobs);
    }

    /**
     * Get total count of all definitions.
     */
    public function count(): int
    {
        return count($this->roles)
            + count($this->permissions)
            + count($this->eventTypes)
            + count($this->subscriptions)
            + count($this->connections)
            + count($this->dispatchPools)
            + count($this->processes)
            + count($this->scheduledJobs);
    }

    /**
     * Convert to array (for JSON serialization).
     *
     * @return array<string, array<array<string, mixed>>>
     */
    public function toArray(): array
    {
        return [
            'roles' => $this->roles,
            'permissions' => $this->permissions,
            'eventTypes' => $this->eventTypes,
            'subscriptions' => $this->subscriptions,
            'connections' => $this->connections,
            'dispatchPools' => $this->dispatchPools,
            'processes' => $this->processes,
            'scheduledJobs' => $this->scheduledJobs,
        ];
    }

    /**
     * Create from array (for JSON deserialization).
     *
     * @param array<string, array<array<string, mixed>>> $data
     */
    public static function fromArray(array $data): self
    {
        return new self(
            roles: $data['roles'] ?? [],
            permissions: $data['permissions'] ?? [],
            eventTypes: $data['eventTypes'] ?? [],
            subscriptions: $data['subscriptions'] ?? [],
            connections: $data['connections'] ?? [],
            dispatchPools: $data['dispatchPools'] ?? [],
            processes: $data['processes'] ?? [],
            scheduledJobs: $data['scheduledJobs'] ?? [],
        );
    }
}
