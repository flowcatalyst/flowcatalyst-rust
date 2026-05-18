<?php

declare(strict_types=1);

namespace FlowCatalyst\Client\Resources;

use FlowCatalyst\Client\FlowCatalystClient;
use FlowCatalyst\DTOs\Application;
use FlowCatalyst\DTOs\ApplicationRole;
use FlowCatalyst\DTOs\ClientConfig;
use FlowCatalyst\DTOs\Requests\ClientConfigRequest;
use FlowCatalyst\DTOs\Requests\CreateApplicationRequest;
use FlowCatalyst\DTOs\Requests\UpdateApplicationRequest;
use FlowCatalyst\DTOs\Responses\ApplicationList;
use FlowCatalyst\DTOs\Responses\ClientConfigList;
use FlowCatalyst\DTOs\ServiceAccount;

class Applications
{
    public function __construct(
        private readonly FlowCatalystClient $client
    ) {}

    /**
     * List applications.
     */
    public function list(?bool $active = null): ApplicationList
    {
        $query = $active !== null
            ? '?' . http_build_query(['active' => $active ? 'true' : 'false'])
            : '';

        $response = $this->client->request('GET', "/api/applications{$query}");

        return ApplicationList::fromArray($response);
    }

    /**
     * Get an application by ID.
     */
    public function get(string $id): Application
    {
        $response = $this->client->request('GET', "/api/applications/{$id}");

        return Application::fromArray($response);
    }

    /**
     * Get an application by code.
     */
    public function getByCode(string $code): Application
    {
        $response = $this->client->request('GET', "/api/applications/by-code/{$code}");

        return Application::fromArray($response);
    }

    /**
     * Create a new application.
     */
    public function create(CreateApplicationRequest $request): Application
    {
        $response = $this->client->request('POST', '/api/applications', [
            'json' => $request->toArray(),
        ]);

        return Application::fromArray($response);
    }

    /**
     * Update an application.
     */
    public function update(string $id, UpdateApplicationRequest $request): Application
    {
        $response = $this->client->request('PUT', "/api/applications/{$id}", [
            'json' => $request->toArray(),
        ]);

        return Application::fromArray($response);
    }

    /**
     * Delete (deactivate) an application.
     */
    public function delete(string $id): void
    {
        $this->client->request('DELETE', "/api/applications/{$id}");
    }

    /**
     * Activate an application.
     */
    public function activate(string $id): Application
    {
        $response = $this->client->request('POST', "/api/applications/{$id}/activate");

        return Application::fromArray($response);
    }

    /**
     * Deactivate an application.
     */
    public function deactivate(string $id): Application
    {
        $response = $this->client->request('POST', "/api/applications/{$id}/deactivate");

        return Application::fromArray($response);
    }

    /**
     * Provision a service account for an application.
     */
    public function provisionServiceAccount(string $id): ServiceAccount
    {
        $response = $this->client->request(
            'POST',
            "/api/applications/{$id}/provision-service-account",
        );

        return ServiceAccount::fromArray($response);
    }

    /**
     * Get the service account attached to an application.
     */
    public function getServiceAccount(string $id): ServiceAccount
    {
        $response = $this->client->request('GET', "/api/applications/{$id}/service-account");

        return ServiceAccount::fromArray($response);
    }

    /**
     * List roles defined for an application (by TSID).
     *
     * Mounted under `/by-id` server-side so the admin TSID lookup doesn't
     * collide with the SDK's `/{app_code}/roles/sync` route.
     *
     * @return ApplicationRole[]
     */
    public function listRoles(string $id): array
    {
        $response = $this->client->request('GET', "/api/applications/by-id/{$id}/roles");

        /** @var array<int, array<string, mixed>> $rows */
        $rows = is_array($response) ? $response : [];
        return array_map(static fn(array $row) => ApplicationRole::fromArray($row), $rows);
    }

    /**
     * List per-client configurations for an application.
     */
    public function listClients(string $id): ClientConfigList
    {
        $response = $this->client->request('GET', "/api/applications/{$id}/clients");

        return ClientConfigList::fromArray($response);
    }

    /**
     * Update per-client config for an application.
     */
    public function updateClientConfig(
        string $id,
        string $clientId,
        ClientConfigRequest $request,
    ): ClientConfig {
        $response = $this->client->request(
            'PUT',
            "/api/applications/{$id}/clients/{$clientId}",
            [
                'json' => $request->toArray(),
            ],
        );

        return ClientConfig::fromArray($response);
    }

    /**
     * Enable an application for a specific client.
     */
    public function enableForClient(string $id, string $clientId): ClientConfig
    {
        $response = $this->client->request(
            'POST',
            "/api/applications/{$id}/clients/{$clientId}/enable",
        );

        return ClientConfig::fromArray($response);
    }

    /**
     * Disable an application for a specific client.
     */
    public function disableForClient(string $id, string $clientId): ClientConfig
    {
        $response = $this->client->request(
            'POST',
            "/api/applications/{$id}/clients/{$clientId}/disable",
        );

        return ClientConfig::fromArray($response);
    }
}
