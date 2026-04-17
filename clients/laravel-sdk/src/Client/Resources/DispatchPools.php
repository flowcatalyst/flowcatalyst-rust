<?php

declare(strict_types=1);

namespace FlowCatalyst\Client\Resources;

use FlowCatalyst\Client\FlowCatalystClient;
use FlowCatalyst\DTOs\DispatchPool;
use FlowCatalyst\DTOs\Requests\CreateDispatchPoolRequest;
use FlowCatalyst\DTOs\Requests\SyncDispatchPoolEntry;
use FlowCatalyst\DTOs\Requests\UpdateDispatchPoolRequest;
use FlowCatalyst\DTOs\Responses\SyncResult;

class DispatchPools
{
    public function __construct(
        private readonly FlowCatalystClient $client
    ) {}

    /**
     * List dispatch pools. The platform returns a bare array (no total).
     *
     * @return DispatchPool[]
     */
    public function list(
        ?string $clientId = null,
        ?string $status = null,
    ): array {
        $queryParams = [];
        if ($clientId !== null) {
            $queryParams['clientId'] = $clientId;
        }
        if ($status !== null) {
            $queryParams['status'] = $status;
        }
        $query = !empty($queryParams) ? '?' . http_build_query($queryParams) : '';
        $response = $this->client->request('GET', "/api/dispatch-pools{$query}");

        /** @var array<int, array<string, mixed>> $rows */
        $rows = is_array($response) && array_is_list($response) ? $response : ($response['pools'] ?? []);
        return array_map(
            fn(array $row) => DispatchPool::fromArray($row),
            $rows,
        );
    }

    /**
     * Get a dispatch pool by ID.
     */
    public function get(string $id): DispatchPool
    {
        $response = $this->client->request('GET', "/api/dispatch-pools/{$id}");

        return DispatchPool::fromArray($response);
    }

    /**
     * Create a new dispatch pool.
     *
     * Returns the created pool's ID. Call `get($id)` if you need the
     * full record.
     */
    public function create(CreateDispatchPoolRequest $request): string
    {
        $response = $this->client->request('POST', '/api/dispatch-pools', [
            'json' => $request->toArray(),
        ]);

        return (string) $response['id'];
    }

    /**
     * Update a dispatch pool's mutable fields. The platform responds
     * with 204 No Content.
     */
    public function update(string $id, UpdateDispatchPoolRequest $request): void
    {
        $this->client->request('PUT', "/api/dispatch-pools/{$id}", [
            'json' => $request->toArray(),
        ]);
    }

    /**
     * Delete (hard-delete) a dispatch pool.
     */
    public function delete(string $id): void
    {
        $this->client->request('DELETE', "/api/dispatch-pools/{$id}");
    }

    /**
     * Archive a dispatch pool (soft status transition).
     */
    public function archive(string $id): DispatchPool
    {
        $response = $this->client->request('POST', "/api/dispatch-pools/{$id}/archive");

        return DispatchPool::fromArray($response);
    }

    /**
     * Suspend a dispatch pool.
     */
    public function suspend(string $id): DispatchPool
    {
        $response = $this->client->request('POST', "/api/dispatch-pools/{$id}/suspend");

        return DispatchPool::fromArray($response);
    }

    /**
     * Activate a dispatch pool (return it to the ACTIVE state).
     */
    public function activate(string $id): DispatchPool
    {
        $response = $this->client->request('POST', "/api/dispatch-pools/{$id}/activate");

        return DispatchPool::fromArray($response);
    }

    /**
     * Sync dispatch pools for an application.
     *
     * Creates/updates anchor-level pools and, when `$removeUnlisted` is
     * true, archives pools not in the sync list.
     *
     * @param SyncDispatchPoolEntry[] $pools
     */
    public function sync(
        string $applicationCode,
        array $pools,
        bool $removeUnlisted = false,
    ): SyncResult {
        $query = $removeUnlisted ? '?removeUnlisted=true' : '';

        $response = $this->client->request(
            'POST',
            "/api/applications/{$applicationCode}/dispatch-pools/sync{$query}",
            [
                'json' => [
                    'pools' => array_map(
                        fn(SyncDispatchPoolEntry $entry) => $entry->toArray(),
                        $pools,
                    ),
                ],
            ],
        );

        return SyncResult::fromArray($response);
    }
}
