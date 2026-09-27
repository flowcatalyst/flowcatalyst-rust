<?php

declare(strict_types=1);

namespace FlowCatalyst\Client\Resources;

use FlowCatalyst\Client\FlowCatalystClient;
use FlowCatalyst\DTOs\Connection;
use FlowCatalyst\DTOs\Requests\CreateConnectionRequest;
use FlowCatalyst\DTOs\Requests\SyncConnectionEntry;
use FlowCatalyst\DTOs\Requests\UpdateConnectionRequest;
use FlowCatalyst\DTOs\Responses\ConnectionList;
use FlowCatalyst\DTOs\Responses\SyncResult;

class Connections
{
    public function __construct(
        private readonly FlowCatalystClient $client
    ) {}

    /**
     * List connections with optional filters.
     *
     * The platform filters on `clientId` and `status` only, so
     * `$serviceAccountId` is applied to the returned connections here (it is
     * still sent, for older platforms); `total` is then the filtered count.
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

        $list = ConnectionList::fromArray($response);
        if ($serviceAccountId === null) {
            return $list;
        }

        $connections = array_values(array_filter(
            $list->connections,
            static fn(Connection $c) => $c->serviceAccountId === $serviceAccountId,
        ));

        return new ConnectionList(connections: $connections, total: count($connections));
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
     * Update a connection. The platform responds with 204 No Content; call
     * `get($id)` if you need the updated record.
     *
     * The platform requires `name` on every update. When `$request->name` is
     * null the connection's current name is read first and sent unchanged.
     */
    public function update(string $id, UpdateConnectionRequest $request): void
    {
        $payload = $request->toArray();
        if (!isset($payload['name'])) {
            $payload = ['name' => $this->get($id)->name] + $payload;
        }

        $this->client->request('PUT', "/api/connections/{$id}", [
            'json' => $payload,
        ]);
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

    /**
     * Sync connections for an application. Creates/updates connections with
     * source=`API` and, when `$removeUnlisted` is true, removes API/CODE
     * -sourced connections not in the sync list. The platform assigns each
     * new connection's service account itself (the application's
     * provisioned one) — never the caller's.
     *
     * @param SyncConnectionEntry[] $connections
     * @param string|null $clientId The client (id or identifier slug) these
     *        connections are scoped to. Null = global (client-less).
     */
    public function sync(
        string $applicationCode,
        array $connections,
        ?string $clientId = null,
        bool $removeUnlisted = false,
    ): SyncResult {
        $query = $removeUnlisted ? '?removeUnlisted=true' : '';

        $body = [
            'connections' => array_map(
                fn(SyncConnectionEntry $entry) => $entry->toArray(),
                $connections,
            ),
        ];
        if ($clientId !== null) {
            $body['clientId'] = $clientId;
        }

        $response = $this->client->request(
            'POST',
            "/api/applications/{$applicationCode}/connections/sync{$query}",
            ['json' => $body],
        );

        return SyncResult::fromArray($response);
    }
}
