<?php

declare(strict_types=1);

namespace FlowCatalyst\Client\Resources;

use FlowCatalyst\Client\FlowCatalystClient;
use FlowCatalyst\DTOs\Process;
use FlowCatalyst\DTOs\Requests\SyncProcessEntry;
use FlowCatalyst\DTOs\Responses\ProcessList;
use FlowCatalyst\DTOs\Responses\SyncResult;

class Processes
{
    public function __construct(
        private readonly FlowCatalystClient $client
    ) {}

    /**
     * List processes. With no filters the platform defaults to
     * `status=CURRENT`, matching the event-types list.
     */
    public function list(
        ?string $application = null,
        ?string $subdomain = null,
        ?string $status = null,
        ?string $search = null,
    ): ProcessList {
        $queryParams = [];
        if ($application !== null) {
            $queryParams['application'] = $application;
        }
        if ($subdomain !== null) {
            $queryParams['subdomain'] = $subdomain;
        }
        if ($status !== null) {
            $queryParams['status'] = $status;
        }
        if ($search !== null) {
            $queryParams['search'] = $search;
        }
        $query = !empty($queryParams) ? '?' . http_build_query($queryParams) : '';
        $response = $this->client->request('GET', "/api/processes{$query}");

        return ProcessList::fromArray($response);
    }

    /**
     * Get a process by ID.
     */
    public function get(string $id): Process
    {
        $response = $this->client->request('GET', "/api/processes/{$id}");

        return Process::fromArray($response);
    }

    /**
     * Get a process by its full three-segment code
     * (`{application}:{subdomain}:{processName}`).
     */
    public function getByCode(string $code): Process
    {
        $response = $this->client->request('GET', "/api/processes/by-code/{$code}");

        return Process::fromArray($response);
    }

    /**
     * Create a new process. Returns the created process's ID. Call
     * `get($id)` if you need the full record.
     *
     * Expected keys: `code`, `name`, optional `description`, optional
     * `body`, optional `diagramType` (defaults to `mermaid`), optional
     * `tags`.
     *
     * @param array<string, mixed> $data
     */
    public function create(array $data): string
    {
        $response = $this->client->request('POST', '/api/processes', [
            'json' => $data,
        ]);

        return (string) $response['id'];
    }

    /**
     * Update a process. The platform responds with 204 No Content.
     *
     * Any subset of `name`, `description`, `body`, `diagramType`, `tags`
     * may be supplied. At least one mutable field must change.
     *
     * @param array<string, mixed> $data
     */
    public function update(string $id, array $data): void
    {
        $this->client->request('PUT', "/api/processes/{$id}", [
            'json' => $data,
        ]);
    }

    /**
     * Archive a process (soft-delete).
     */
    public function archive(string $id): void
    {
        $this->client->request('POST', "/api/processes/{$id}/archive");
    }

    /**
     * Hard-delete a process. The platform requires the process to be
     * archived first.
     */
    public function delete(string $id): void
    {
        $this->client->request('DELETE', "/api/processes/{$id}");
    }

    /**
     * Sync processes for an application. Creates/updates processes whose
     * source is `CODE`/`API` and, when `$removeUnlisted` is true, archives
     * CODE/API-sourced processes not in the sync list. UI-sourced rows are
     * left untouched.
     *
     * @param SyncProcessEntry[] $processes
     */
    public function sync(
        string $applicationCode,
        array $processes,
        bool $removeUnlisted = false,
    ): SyncResult {
        $query = $removeUnlisted ? '?removeUnlisted=true' : '';
        $appCode = rawurlencode($applicationCode);

        $response = $this->client->request(
            'POST',
            "/api/applications/{$appCode}/processes/sync{$query}",
            [
                'json' => [
                    'processes' => array_map(
                        fn(SyncProcessEntry $entry) => $entry->toArray(),
                        $processes,
                    ),
                ],
            ],
        );

        return SyncResult::fromArray($response);
    }
}
