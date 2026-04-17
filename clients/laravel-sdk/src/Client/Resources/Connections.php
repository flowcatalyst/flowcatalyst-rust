<?php

declare(strict_types=1);

namespace FlowCatalyst\Client\Resources;

use FlowCatalyst\Client\FlowCatalystClient;
use FlowCatalyst\DTOs\Connection;
use FlowCatalyst\DTOs\Requests\CreateConnectionRequest;
use FlowCatalyst\DTOs\Requests\UpdateConnectionRequest;
use FlowCatalyst\DTOs\Responses\ConnectionList;

class Connections
{
    public function __construct(
        private readonly FlowCatalystClient $client
    ) {}

    /**
     * List connections with optional filters.
     */
    public function list(
        ?string $clientId = null,
        ?string $status = null,
        ?string $serviceAccountId = null,
    ): ConnectionList {
        $queryParams = [];
        if ($clientId !== null) {
            $queryParams['clientId'] = $clientId;
        }
        if ($status !== null) {
            $queryParams['status'] = $status;
        }
        if ($serviceAccountId !== null) {
            $queryParams['serviceAccountId'] = $serviceAccountId;
        }
        $query = !empty($queryParams) ? '?' . http_build_query($queryParams) : '';
        $response = $this->client->request('GET', "/api/connections{$query}");

        return ConnectionList::fromArray($response);
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
     * Returns the created connection's ID. Call `get($id)` if you need
     * the full record.
     */
    public function create(CreateConnectionRequest $request): string
    {
        $response = $this->client->request('POST', '/api/connections', [
            'json' => $request->toArray(),
        ]);

        return (string) $response['id'];
    }

    /**
     * Update a connection.
     */
    public function update(string $id, UpdateConnectionRequest $request): Connection
    {
        $response = $this->client->request('PUT', "/api/connections/{$id}", [
            'json' => $request->toArray(),
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
