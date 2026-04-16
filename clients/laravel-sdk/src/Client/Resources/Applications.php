<?php

declare(strict_types=1);

namespace FlowCatalyst\Client\Resources;

use FlowCatalyst\Client\FlowCatalystClient;
use FlowCatalyst\DTOs\Application;

class Applications
{
    public function __construct(
        private readonly FlowCatalystClient $client
    ) {}

    /**
     * List all applications.
     *
     * @return array{applications: Application[], total: int}
     */
    public function list(): array
    {
        $response = $this->client->request('GET', '/api/applications');

        return [
            'applications' => array_map(
                fn(array $item) => Application::fromArray($item),
                $response['applications'] ?? []
            ),
            'total' => $response['total'] ?? 0,
        ];
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
     *
     * @param array{
     *     code: string,
     *     name: string,
     *     description?: string,
     *     defaultBaseUrl?: string,
     *     iconUrl?: string
     * } $data
     */
    public function create(array $data): Application
    {
        $response = $this->client->request('POST', '/api/applications', [
            'json' => $data,
        ]);

        return Application::fromArray($response);
    }

    /**
     * Update an application.
     *
     * @param array{
     *     name?: string,
     *     description?: string,
     *     defaultBaseUrl?: string,
     *     iconUrl?: string
     * } $data
     */
    public function update(string $id, array $data): Application
    {
        $response = $this->client->request('PUT', "/api/applications/{$id}", [
            'json' => $data,
        ]);

        return Application::fromArray($response);
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
}
