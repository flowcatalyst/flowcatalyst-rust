<?php

declare(strict_types=1);

namespace FlowCatalyst\Client\Resources;

use FlowCatalyst\Client\FlowCatalystClient;
use FlowCatalyst\DTOs\Client;

class Clients
{
    public function __construct(
        private readonly FlowCatalystClient $client
    ) {}

    /**
     * List all clients.
     *
     * @return array{clients: Client[], total: int}
     */
    public function list(): array
    {
        $response = $this->client->request('GET', '/api/clients');

        return [
            'clients' => array_map(
                fn(array $item) => Client::fromArray($item),
                $response['clients'] ?? []
            ),
            'total' => $response['total'] ?? 0,
        ];
    }

    /**
     * Get a client by ID.
     */
    public function get(string $id): Client
    {
        $response = $this->client->request('GET', "/api/clients/{$id}");

        return Client::fromArray($response);
    }

    /**
     * Create a new client.
     *
     * @param array{
     *     name: string,
     *     identifier: string
     * } $data
     */
    public function create(array $data): Client
    {
        $response = $this->client->request('POST', '/api/clients', [
            'json' => $data,
        ]);

        return Client::fromArray($response);
    }

    /**
     * Update a client.
     *
     * @param array{
     *     name?: string
     * } $data
     */
    public function update(string $id, array $data): Client
    {
        $response = $this->client->request('PUT', "/api/clients/{$id}", [
            'json' => $data,
        ]);

        return Client::fromArray($response);
    }

    /**
     * Activate a client.
     */
    public function activate(string $id): Client
    {
        $response = $this->client->request('POST', "/api/clients/{$id}/activate");

        return Client::fromArray($response);
    }

    /**
     * Deactivate a client.
     */
    public function deactivate(string $id): Client
    {
        $response = $this->client->request('POST', "/api/clients/{$id}/deactivate");

        return Client::fromArray($response);
    }

    /**
     * Suspend a client with a reason.
     */
    public function suspend(string $id, string $reason): Client
    {
        $response = $this->client->request('POST', "/api/clients/{$id}/suspend", [
            'json' => ['reason' => $reason],
        ]);

        return Client::fromArray($response);
    }

    /**
     * Get applications configured for a client.
     *
     * @return array{applications: array<array{id: string, code: string, name: string, enabled: bool}>}
     */
    public function getApplications(string $id): array
    {
        return $this->client->request('GET', "/api/clients/{$id}/applications");
    }

    /**
     * Update the applications configured for a client.
     *
     * @param array{applicationIds: string[]} $data
     * @return array{applications: array<array{id: string, code: string, name: string, enabled: bool}>}
     */
    public function updateApplications(string $id, array $data): array
    {
        return $this->client->request('PUT', "/api/clients/{$id}/applications", [
            'json' => $data,
        ]);
    }

    /**
     * Enable an application for a client.
     */
    public function enableApplication(string $clientId, string $applicationId): void
    {
        $this->client->request('POST', "/api/clients/{$clientId}/applications/{$applicationId}/enable");
    }

    /**
     * Disable an application for a client.
     */
    public function disableApplication(string $clientId, string $applicationId): void
    {
        $this->client->request('POST', "/api/clients/{$clientId}/applications/{$applicationId}/disable");
    }
}
