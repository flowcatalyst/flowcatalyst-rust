<?php

declare(strict_types=1);

namespace FlowCatalyst\Client\Resources;

use FlowCatalyst\Client\FlowCatalystClient;
use FlowCatalyst\DTOs\Requests\CreateScheduledJobRequest;
use FlowCatalyst\DTOs\Requests\SyncScheduledJobEntry;
use FlowCatalyst\DTOs\Requests\UpdateScheduledJobRequest;
use FlowCatalyst\DTOs\ScheduledJob;
use FlowCatalyst\DTOs\ScheduledJobInstance;
use FlowCatalyst\DTOs\ScheduledJobInstanceLog;

/**
 * Scheduled Jobs resource — CRUD + state transitions + history reads +
 * the SDK callback paths (`logForInstance`, `completeInstance`).
 *
 * Mirrors the TypeScript SDK's `ScheduledJobsResource`.
 */
class ScheduledJobs
{
    public function __construct(
        private readonly FlowCatalystClient $client,
    ) {}

    // ── CRUD ──────────────────────────────────────────────────────────────

    /**
     * Create a new scheduled job. Returns the new job's id.
     */
    public function create(CreateScheduledJobRequest $request): string
    {
        $response = $this->client->request('POST', '/api/scheduled-jobs', [
            'json' => $request->toArray(),
        ]);
        return (string) $response['id'];
    }

    /**
     * List scheduled jobs. Returns the paginated payload (data + page + size +
     * total + totalPages; the platform sends the last as `total_pages`); typed
     * DTOs are exposed via `data`.
     *
     * @return array{
     *   data: ScheduledJob[],
     *   page: int,
     *   size: int,
     *   total: int,
     *   totalPages: int,
     * }
     */
    public function list(
        ?string $clientId = null,
        ?string $status = null,
        ?string $search = null,
        int $page = 0,
        int $size = 20,
    ): array {
        $query = http_build_query(array_filter([
            'clientId' => $clientId,
            'status' => $status,
            'search' => $search,
            'page' => $page,
            'size' => $size,
        ], static fn($v) => $v !== null && $v !== ''));
        $response = $this->client->request('GET', '/api/scheduled-jobs?' . $query);

        return [
            'data' => array_map(
                static fn(array $row) => ScheduledJob::fromArray($row),
                (array) ($response['data'] ?? []),
            ),
            'page' => (int) ($response['page'] ?? 0),
            'size' => (int) ($response['size'] ?? $size),
            'total' => (int) ($response['total'] ?? 0),
            'totalPages' => (int) ($response['total_pages'] ?? $response['totalPages'] ?? 0),
        ];
    }

    public function get(string $id): ScheduledJob
    {
        $response = $this->client->request('GET', "/api/scheduled-jobs/{$id}");
        return ScheduledJob::fromArray($response);
    }

    public function getByCode(string $code, ?string $clientId = null): ScheduledJob
    {
        $query = $clientId !== null ? '?' . http_build_query(['clientId' => $clientId]) : '';
        $response = $this->client->request('GET', "/api/scheduled-jobs/by-code/{$code}{$query}");
        return ScheduledJob::fromArray($response);
    }

    public function update(string $id, UpdateScheduledJobRequest $request): void
    {
        $this->client->request('PUT', "/api/scheduled-jobs/{$id}", [
            'json' => $request->toArray(),
        ]);
    }

    // ── State transitions ─────────────────────────────────────────────────

    public function pause(string $id): void
    {
        $this->client->request('POST', "/api/scheduled-jobs/{$id}/pause");
    }

    public function resume(string $id): void
    {
        $this->client->request('POST', "/api/scheduled-jobs/{$id}/resume");
    }

    public function archive(string $id): void
    {
        $this->client->request('POST', "/api/scheduled-jobs/{$id}/archive");
    }

    public function delete(string $id): void
    {
        $this->client->request('DELETE', "/api/scheduled-jobs/{$id}");
    }

    /**
     * Manually fire a scheduled job. Returns the new instance's id.
     */
    public function fire(string $id, ?string $correlationId = null): string
    {
        $body = [];
        if ($correlationId !== null) $body['correlationId'] = $correlationId;
        $response = $this->client->request('POST', "/api/scheduled-jobs/{$id}/fire", [
            'json' => (object) $body,
        ]);
        return (string) $response['id'];
    }

    // ── Instance reads ────────────────────────────────────────────────────

    /**
     * List a job's instances, newest first.
     *
     * The platform filters on `status` only. `$triggerKind`, `$from` and `$to`
     * are sent for older platforms but have no effect on the current one.
     *
     * @return array{
     *   data: ScheduledJobInstance[],
     *   page: int,
     *   size: int,
     *   total: int,
     *   totalPages: int,
     * }
     */
    public function listInstances(
        string $jobId,
        ?string $status = null,
        ?string $triggerKind = null,
        ?string $from = null,
        ?string $to = null,
        int $page = 0,
        int $size = 20,
    ): array {
        $query = http_build_query(array_filter([
            'status' => $status,
            'triggerKind' => $triggerKind,
            'from' => $from,
            'to' => $to,
            'page' => $page,
            'size' => $size,
        ], static fn($v) => $v !== null && $v !== ''));
        $response = $this->client->request(
            'GET',
            "/api/scheduled-jobs/{$jobId}/instances?{$query}",
        );

        return [
            'data' => array_map(
                static fn(array $row) => ScheduledJobInstance::fromArray($row),
                (array) ($response['data'] ?? []),
            ),
            'page' => (int) ($response['page'] ?? 0),
            'size' => (int) ($response['size'] ?? $size),
            'total' => (int) ($response['total'] ?? 0),
            'totalPages' => (int) ($response['total_pages'] ?? $response['totalPages'] ?? 0),
        ];
    }

    public function getInstance(string $instanceId): ScheduledJobInstance
    {
        $response = $this->client->request('GET', "/api/scheduled-jobs/instances/{$instanceId}");
        return ScheduledJobInstance::fromArray($response);
    }

    /**
     * @return ScheduledJobInstanceLog[]
     */
    public function listInstanceLogs(string $instanceId): array
    {
        $response = $this->client->request('GET', "/api/scheduled-jobs/instances/{$instanceId}/logs");
        return array_map(
            static fn(array $row) => ScheduledJobInstanceLog::fromArray($row),
            (array) $response,
        );
    }

    // ── SDK callback paths ────────────────────────────────────────────────

    /**
     * Append a log entry to an instance.
     */
    public function logForInstance(
        string $instanceId,
        string $message,
        string $level = 'INFO',
        mixed $metadata = null,
    ): void {
        $body = ['message' => $message, 'level' => $level];
        if ($metadata !== null) $body['metadata'] = $metadata;
        $this->client->request('POST', "/api/scheduled-jobs/instances/{$instanceId}/log", [
            'json' => $body,
        ]);
    }

    /**
     * Mark an instance as complete (only meaningful when the job has
     * `tracksCompletion = true`).
     */
    public function completeInstance(
        string $instanceId,
        string $status,
        mixed $result = null,
    ): void {
        $body = ['status' => $status];
        if ($result !== null) $body['result'] = $result;
        $this->client->request('POST', "/api/scheduled-jobs/instances/{$instanceId}/complete", [
            'json' => $body,
        ]);
    }

    // ── Sync ──────────────────────────────────────────────────────────────

    /**
     * Sync the scheduled-job definitions for an application. The platform
     * upserts by `(client_id, code)`; with `archiveUnlisted = true`, jobs
     * present in the platform but missing from this list are archived.
     *
     * @param SyncScheduledJobEntry[] $jobs
     * @return array{created: string[], updated: string[], archived: string[]}
     */
    public function sync(
        string $applicationCode,
        array $jobs,
        ?string $clientId = null,
        bool $archiveUnlisted = false,
    ): array {
        $body = [
            'jobs' => array_map(
                static fn(SyncScheduledJobEntry $entry) => $entry->toArray(),
                $jobs,
            ),
            'archiveUnlisted' => $archiveUnlisted,
        ];
        if ($clientId !== null) $body['clientId'] = $clientId;

        $response = $this->client->request(
            'POST',
            "/api/applications/{$applicationCode}/scheduled-jobs/sync",
            ['json' => $body],
        );

        return [
            'created' => array_map('strval', (array) ($response['created'] ?? [])),
            'updated' => array_map('strval', (array) ($response['updated'] ?? [])),
            'archived' => array_map('strval', (array) ($response['archived'] ?? [])),
        ];
    }
}
