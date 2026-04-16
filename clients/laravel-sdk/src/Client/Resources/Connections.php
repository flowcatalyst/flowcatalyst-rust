<?php

declare(strict_types=1);

namespace FlowCatalyst\Client\Resources;

use FlowCatalyst\Client\FlowCatalystClient;
use FlowCatalyst\DTOs\Connection;
use FlowCatalyst\Enums\ConnectionStatus;

class Connections
{
    public function __construct(
        private readonly FlowCatalystClient $client
    ) {}

    /**
     * List all connections with optional filters.
     *
     * @param array $filters Optional filters: clientId, status, serviceAccountId
     * @return array{connections: Connection[], total: int}
     */
    public function list(array $filters = []): array
    {
        $query = http_build_query($filters);
        $endpoint = '/api/connections' . ($query ? "?{$query}" : '');

        $response = $this->client->request('GET', $endpoint);

        return [
            'connections' => array_map(
                fn(array $item) => Connection::fromArray($item),
                $response['connections'] ?? []
            ),
            'total' => $response['total'] ?? count($response['connections'] ?? []),
        ];
    }

    /**
     * Get a connection by ID.
     */
    public function get(string $id): Connection
    {
        $response = $this->client->request('GET', "/api/connections/{$id}");

        return Connection::fromArray($response);
    }

    /**
     * Create a new connection.
     *
     * @param array{
     *     code: string,
     *     name: string,
     *     description?: string,
     *     endpoint: string,
     *     externalId?: string,
     *     serviceAccountId: string,
     *     clientId?: string
     * } $data
     */
    public function create(array $data): Connection
    {
        $response = $this->client->request('POST', '/api/connections', [
            'json' => $data,
        ]);

        return Connection::fromArray($response);
    }

    /**
     * Update a connection.
     *
     * @param array{
     *     name?: string,
     *     description?: string|null,
     *     endpoint?: string,
     *     externalId?: string|null,
     *     status?: ConnectionStatus|string
     * } $data
     */
    public function update(string $id, array $data): Connection
    {
        // Convert enum to string
        if (isset($data['status']) && $data['status'] instanceof ConnectionStatus) {
            $data['status'] = $data['status']->value;
        }

        $response = $this->client->request('PUT', "/api/connections/{$id}", [
            'json' => $data,
        ]);

        return Connection::fromArray($response);
    }

    /**
     * Delete a connection.
     */
    public function delete(string $id): void
    {
        $this->client->request('DELETE', "/api/connections/{$id}");
    }

    /**
     * Pause a connection.
     */
    public function pause(string $id): Connection
    {
        $response = $this->client->request('POST', "/api/connections/{$id}/pause");

        return Connection::fromArray($response);
    }

    /**
     * Activate a connection.
     */
    public function activate(string $id): Connection
    {
        $response = $this->client->request('POST', "/api/connections/{$id}/activate");

        return Connection::fromArray($response);
    }
}
