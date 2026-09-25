<?php

declare(strict_types=1);

namespace FlowCatalyst\Sync;

/**
 * Result of a sync operation for a single application.
 */
final class SyncResult
{
    /**
     * @param string $applicationCode The application that was synced
     * @param array{created: int, updated: int, deleted: int, error?: string} $roles Role sync results
     * @param array{created: int, updated: int, deleted: int, error?: string} $eventTypes Event type sync results
     * @param array{created: int, updated: int, deleted: int, error?: string} $subscriptions Subscription sync results
     * @param array{created: int, updated: int, deleted: int, error?: string} $connections Connection sync results
     * @param array{created: int, updated: int, deleted: int, error?: string} $dispatchPools Dispatch pool sync results
     * @param array{created: int, updated: int, deleted: int, error?: string} $principals Principal sync results
     * @param array{created: int, updated: int, deleted: int, error?: string} $processes Process sync results
     * @param array{created: int, updated: int, deleted: int, error?: string} $scheduledJobs Scheduled-job sync results
     * @param array{created: int, updated: int, deleted: int, error?: string, version?: string} $openapi OpenAPI-document publish result
     */
    public function __construct(
        public readonly string $applicationCode,
        public readonly array $roles = ['created' => 0, 'updated' => 0, 'deleted' => 0],
        public readonly array $eventTypes = ['created' => 0, 'updated' => 0, 'deleted' => 0],
        public readonly array $subscriptions = ['created' => 0, 'updated' => 0, 'deleted' => 0],
        public readonly array $dispatchPools = ['created' => 0, 'updated' => 0, 'deleted' => 0],
        public readonly array $principals = ['created' => 0, 'updated' => 0, 'deleted' => 0],
        public readonly array $processes = ['created' => 0, 'updated' => 0, 'deleted' => 0],
        public readonly array $scheduledJobs = ['created' => 0, 'updated' => 0, 'deleted' => 0],
        public readonly array $openapi = ['created' => 0, 'updated' => 0, 'deleted' => 0],
        public readonly array $connections = ['created' => 0, 'updated' => 0, 'deleted' => 0],
    ) {}

    /**
     * Check if any roles were synced.
     */
    public function hasRoleChanges(): bool
    {
        return ($this->roles['created'] ?? 0) > 0
            || ($this->roles['updated'] ?? 0) > 0
            || ($this->roles['deleted'] ?? 0) > 0;
    }

    /**
     * Check if any event types were synced.
     */
    public function hasEventTypeChanges(): bool
    {
        return ($this->eventTypes['created'] ?? 0) > 0
            || ($this->eventTypes['updated'] ?? 0) > 0
            || ($this->eventTypes['deleted'] ?? 0) > 0;
    }

    /**
     * Check if any subscriptions were synced.
     */
    public function hasSubscriptionChanges(): bool
    {
        return ($this->subscriptions['created'] ?? 0) > 0
            || ($this->subscriptions['updated'] ?? 0) > 0
            || ($this->subscriptions['deleted'] ?? 0) > 0;
    }

    /**
     * Check if any connections were synced.
     */
    public function hasConnectionChanges(): bool
    {
        return ($this->connections['created'] ?? 0) > 0
            || ($this->connections['updated'] ?? 0) > 0
            || ($this->connections['deleted'] ?? 0) > 0;
    }

    /**
     * Check if any dispatch pools were synced.
     */
    public function hasDispatchPoolChanges(): bool
    {
        return ($this->dispatchPools['created'] ?? 0) > 0
            || ($this->dispatchPools['updated'] ?? 0) > 0
            || ($this->dispatchPools['deleted'] ?? 0) > 0;
    }

    /**
     * Check if any principals were synced.
     */
    public function hasPrincipalChanges(): bool
    {
        return ($this->principals['created'] ?? 0) > 0
            || ($this->principals['updated'] ?? 0) > 0
            || ($this->principals['deleted'] ?? 0) > 0;
    }

    /**
     * Check if any processes were synced.
     */
    public function hasProcessChanges(): bool
    {
        return ($this->processes['created'] ?? 0) > 0
            || ($this->processes['updated'] ?? 0) > 0
            || ($this->processes['deleted'] ?? 0) > 0;
    }

    /**
     * Check if any scheduled jobs were synced.
     */
    public function hasScheduledJobChanges(): bool
    {
        return ($this->scheduledJobs['created'] ?? 0) > 0
            || ($this->scheduledJobs['updated'] ?? 0) > 0
            || ($this->scheduledJobs['deleted'] ?? 0) > 0;
    }

    /**
     * Check if the OpenAPI document was published.
     */
    public function hasOpenapiChanges(): bool
    {
        return ($this->openapi['created'] ?? 0) > 0
            || ($this->openapi['updated'] ?? 0) > 0
            || ($this->openapi['deleted'] ?? 0) > 0;
    }

    /**
     * Check if any changes were made.
     */
    public function hasChanges(): bool
    {
        return $this->hasRoleChanges()
            || $this->hasEventTypeChanges()
            || $this->hasSubscriptionChanges()
            || $this->hasConnectionChanges()
            || $this->hasDispatchPoolChanges()
            || $this->hasPrincipalChanges()
            || $this->hasProcessChanges()
            || $this->hasScheduledJobChanges()
            || $this->hasOpenapiChanges();
    }

    /**
     * Check if there were any errors.
     */
    public function hasErrors(): bool
    {
        return isset($this->roles['error'])
            || isset($this->eventTypes['error'])
            || isset($this->subscriptions['error'])
            || isset($this->connections['error'])
            || isset($this->dispatchPools['error'])
            || isset($this->principals['error'])
            || isset($this->processes['error'])
            || isset($this->scheduledJobs['error'])
            || isset($this->openapi['error']);
    }

    /**
     * Get all error messages.
     *
     * @return string[]
     */
    public function getErrors(): array
    {
        $errors = [];

        if (isset($this->roles['error'])) {
            $errors['roles'] = $this->roles['error'];
        }

        if (isset($this->eventTypes['error'])) {
            $errors['eventTypes'] = $this->eventTypes['error'];
        }

        if (isset($this->subscriptions['error'])) {
            $errors['subscriptions'] = $this->subscriptions['error'];
        }

        if (isset($this->connections['error'])) {
            $errors['connections'] = $this->connections['error'];
        }

        if (isset($this->dispatchPools['error'])) {
            $errors['dispatchPools'] = $this->dispatchPools['error'];
        }

        if (isset($this->principals['error'])) {
            $errors['principals'] = $this->principals['error'];
        }

        if (isset($this->processes['error'])) {
            $errors['processes'] = $this->processes['error'];
        }

        if (isset($this->scheduledJobs['error'])) {
            $errors['scheduledJobs'] = $this->scheduledJobs['error'];
        }

        if (isset($this->openapi['error'])) {
            $errors['openapi'] = $this->openapi['error'];
        }

        return $errors;
    }

    /**
     * Get total counts across all definition types.
     *
     * @return array{created: int, updated: int, deleted: int}
     */
    public function getTotals(): array
    {
        return [
            'created' => ($this->roles['created'] ?? 0)
                + ($this->eventTypes['created'] ?? 0)
                + ($this->subscriptions['created'] ?? 0)
                + ($this->connections['created'] ?? 0)
                + ($this->dispatchPools['created'] ?? 0)
                + ($this->principals['created'] ?? 0)
                + ($this->processes['created'] ?? 0)
                + ($this->scheduledJobs['created'] ?? 0)
                + ($this->openapi['created'] ?? 0),
            'updated' => ($this->roles['updated'] ?? 0)
                + ($this->eventTypes['updated'] ?? 0)
                + ($this->subscriptions['updated'] ?? 0)
                + ($this->connections['updated'] ?? 0)
                + ($this->dispatchPools['updated'] ?? 0)
                + ($this->principals['updated'] ?? 0)
                + ($this->processes['updated'] ?? 0)
                + ($this->scheduledJobs['updated'] ?? 0)
                + ($this->openapi['updated'] ?? 0),
            'deleted' => ($this->roles['deleted'] ?? 0)
                + ($this->eventTypes['deleted'] ?? 0)
                + ($this->subscriptions['deleted'] ?? 0)
                + ($this->connections['deleted'] ?? 0)
                + ($this->dispatchPools['deleted'] ?? 0)
                + ($this->principals['deleted'] ?? 0)
                + ($this->processes['deleted'] ?? 0)
                + ($this->scheduledJobs['deleted'] ?? 0)
                + ($this->openapi['deleted'] ?? 0),
        ];
    }

    /**
     * Create an empty result for an application.
     */
    public static function empty(string $applicationCode): self
    {
        return new self($applicationCode);
    }

    /**
     * Combine this result with another for the SAME application. Counts are
     * summed per type; errors from both sides are concatenated (`; `-joined)
     * rather than one overwriting the other.
     *
     * NOT how `DefinitionSynchronizer::syncGrouped()` reconciles more than
     * one `SyncDefinitionSet` sharing an application code — the platform
     * scopes `removeUnlisted` to one (application, client) PER CALL, so
     * syncing each set separately and merging the RESULTS would still let
     * the second call delete what the first just created.
     * {@see SyncDefinitionSet::merge()} folds the SETS themselves into one
     * before a single `sync()` call instead. This method remains a general
     * purpose combinator for a caller that legitimately has two independent
     * `SyncResult`s for the same application to report on together.
     */
    public function merge(self $other): self
    {
        $sum = static function (array $a, array $b): array {
            $out = [
                'created' => ($a['created'] ?? 0) + ($b['created'] ?? 0),
                'updated' => ($a['updated'] ?? 0) + ($b['updated'] ?? 0),
                'deleted' => ($a['deleted'] ?? 0) + ($b['deleted'] ?? 0),
            ];
            $errors = array_filter([$a['error'] ?? null, $b['error'] ?? null]);
            if ($errors !== []) {
                $out['error'] = implode('; ', $errors);
            }
            return $out;
        };

        return new self(
            applicationCode: $this->applicationCode !== '' ? $this->applicationCode : $other->applicationCode,
            roles: $sum($this->roles, $other->roles),
            eventTypes: $sum($this->eventTypes, $other->eventTypes),
            subscriptions: $sum($this->subscriptions, $other->subscriptions),
            dispatchPools: $sum($this->dispatchPools, $other->dispatchPools),
            principals: $sum($this->principals, $other->principals),
            processes: $sum($this->processes, $other->processes),
            scheduledJobs: $sum($this->scheduledJobs, $other->scheduledJobs),
            openapi: $sum($this->openapi, $other->openapi),
            connections: $sum($this->connections, $other->connections),
        );
    }
}
