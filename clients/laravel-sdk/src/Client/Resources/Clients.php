<?php

declare(strict_types=1);

namespace FlowCatalyst\Client\Resources;

use FlowCatalyst\Client\FlowCatalystClient;
use FlowCatalyst\DTOs\Client;
use FlowCatalyst\DTOs\ClientApplication;
use FlowCatalyst\DTOs\Requests\CreateClientRequest;
use FlowCatalyst\DTOs\Requests\UpdateClientApplicationsRequest;
use FlowCatalyst\DTOs\Requests\UpdateClientRequest;
use FlowCatalyst\DTOs\Responses\ClientList;

class Clients
{
    public function __construct(
        private readonly FlowCatalystClient $client
    ) {}

    /**
     * List clients.
     */
    public function list(?string $status = null): ClientList
    {
        $query = $status !== null
            ? '?' . http_build_query(['status' => $status])
            : '';
        $response = $this->client->request('GET', "/api/clients{$query}");

        return ClientList::fromArray($response);
    }

    /**
     * Search clients by name or identifier.
     */
    public function search(string $term): ClientList
    {
        $query = '?' . http_build_query(['q' => $term]);
        $response = $this->client->request('GET', "/api/clients/search{$query}");

        return ClientList::fromArray($response);
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
     * Get a client by its identifier (slug).
     */
    public function getByIdentifier(string $identifier): Client
    {
        $response = $this->client->request('GET', "/api/clients/by-identifier/{$identifier}");

        return Client::fromArray($response);
    }

    /**
     * Create a new client.
     *
     * Returns the created client's ID. The platform's create endpoint
     * returns `{id}` only — use `get($id)` to fetch the full record.
     */
    public function create(CreateClientRequest $request): string
    {
        $response = $this->client->request('POST', '/api/clients', [
            'json' => $request->toArray(),
        ]);

        return (string) $response['id'];
    }

    /**
     * Update a client.
     */
    public function update(string $id, UpdateClientRequest $request): Client
    {
        $response = $this->client->request('PUT', "/api/clients/{$id}", [
            'json' => $request->toArray(),
        ]);

        return Client::fromArray($response);
    }

    /**
     * Activate a client.
     *
     * The platform responds with `{message}` only. Call `get($id)` if you
     * need the refreshed record.
     */
    public function activate(string $id): void
    {
        $this->client->request('POST', "/api/clients/{$id}/activate");
    }

    /**
     * Suspend a client. A reason is required.
     */
    public function suspend(string $id, string $reason): void
    {
        $this->client->request('POST', "/api/clients/{$id}/suspend", [
            'json' => ['reason' => $reason],
        ]);
    }

    /**
     * Deactivate a client. A reason is required.
     */
    public function deactivate(string $id, string $reason): void
    {
        $this->client->request('POST', "/api/clients/{$id}/deactivate", [
            'json' => ['reason' => $reason],
        ]);
    }

    /**
     * Add a note to a client's audit history.
     */
    public function addNote(string $id, string $category, string $text): void
    {
        $this->client->request('POST', "/api/clients/{$id}/notes", [
            'json' => [
                'category' => $category,
                'text' => $text,
            ],
        ]);
    }

    /**
     * List applications configured for a client.
     *
     * @return ClientApplication[]
     */
    public function getApplications(string $id): array
    {
        $response = $this->client->request('GET', "/api/clients/{$id}/applications");

        /** @var array<int, array<string, mixed>> $rows */
        $rows = $response['applications'] ?? [];
        return array_map(
            fn(array $row) => ClientApplication::fromArray($row),
            $rows,
        );
    }

    /**
     * Declaratively set the applications enabled for a client.
     */
    public function updateApplications(
        string $id,
        UpdateClientApplicationsRequest $request,
    ): void {
        $this->client->request('PUT', "/api/clients/{$id}/applications", [
            'json' => $request->toArray(),
        ]);
    }

    /**
     * Enable a single application for a client.
     */
    public function enableApplication(string $clientId, string $applicationId): void
    {
        $this->client->request('POST', "/api/clients/{$clientId}/applications/{$applicationId}/enable");
    }

    /**
     * Disable a single application for a client.
     */
    public function disableApplication(string $clientId, string $applicationId): void
    {
        $this->client->request('POST', "/api/clients/{$clientId}/applications/{$applicationId}/disable");
    }
}
