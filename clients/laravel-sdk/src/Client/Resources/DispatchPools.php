<?php

declare(strict_types=1);

namespace FlowCatalyst\Client\Resources;

use FlowCatalyst\Client\FlowCatalystClient;
use FlowCatalyst\DTOs\DispatchPool;
use FlowCatalyst\Enums\DispatchPoolStatus;

class DispatchPools
{
    public function __construct(
        private readonly FlowCatalystClient $client
    ) {}

    /**
     * List all dispatch pools with optional filters.
     *
     * @param array $filters Optional filters: clientId, status, anchorLevel
     * @return array{pools: DispatchPool[], total: int}
     */
    public function list(array $filters = []): array
    {
        $query = http_build_query($filters);
        $endpoint = '/api/dispatch-pools' . ($query ? "?{$query}" : '');

        $response = $this->client->request('GET', $endpoint);

        return [
            'pools' => array_map(
                fn(array $item) => DispatchPool::fromArray($item),
                $response['pools'] ?? []
            ),
            'total' => $response['total'] ?? count($response['pools'] ?? []),
        ];
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
     * @param array{
     *     code: string,
     *     name: string,
     *     description?: string,
     *     rateLimit: int,
     *     concurrency: int,
     *     clientId?: string
     * } $data
     */
    public function create(array $data): DispatchPool
    {
        $response = $this->client->request('POST', '/api/dispatch-pools', [
            'json' => $data,
        ]);

        return DispatchPool::fromArray($response);
    }

    /**
     * Update a dispatch pool.
     *
     * @param array{
     *     name?: string,
     *     description?: string,
     *     rateLimit?: int,
     *     concurrency?: int,
     *     status?: DispatchPoolStatus|string
     * } $data
     */
    public function update(string $id, array $data): DispatchPool
    {
        // Convert enum to string
        if (isset($data['status']) && $data['status'] instanceof DispatchPoolStatus) {
            $data['status'] = $data['status']->value;
        }

        $response = $this->client->request('PUT', "/api/dispatch-pools/{$id}", [
            'json' => $data,
        ]);

        return DispatchPool::fromArray($response);
    }

    /**
     * Delete (archive) a dispatch pool.
     */
    public function delete(string $id): void
    {
        $this->client->request('DELETE', "/api/dispatch-pools/{$id}");
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
     * Activate a dispatch pool.
     */
    public function activate(string $id): DispatchPool
    {
        $response = $this->client->request('POST', "/api/dispatch-pools/{$id}/activate");

        return DispatchPool::fromArray($response);
    }

    /**
     * Sync dispatch pools for an application.
     *
     * This creates/updates anchor-level pools and optionally removes
     * pools not in the sync list.
     *
     * @param string $appCode The application code
     * @param array<array{
     *     code: string,
     *     name?: string,
     *     description?: string,
     *     rateLimit?: int,
     *     concurrency?: int
     * }> $pools The pools to sync
     * @param bool $removeUnlisted If true, archives pools not in the list
     * @return array{created: int, updated: int, deleted: int, pools: DispatchPool[]}
     */
    public function sync(string $appCode, array $pools, bool $removeUnlisted = false): array
    {
        $query = $removeUnlisted ? '?removeUnlisted=true' : '';

        $response = $this->client->request('POST', "/api/applications/{$appCode}/dispatch-pools/sync{$query}", [
            'json' => ['pools' => $pools],
        ]);

        return [
            'created' => $response['created'] ?? 0,
            'updated' => $response['updated'] ?? 0,
            'deleted' => $response['deleted'] ?? 0,
            'pools' => array_map(
                fn(array $item) => DispatchPool::fromArray($item),
                $response['pools'] ?? []
            ),
        ];
    }
}
