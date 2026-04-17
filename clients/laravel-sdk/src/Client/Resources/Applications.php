<?php

declare(strict_types=1);

namespace FlowCatalyst\Client\Resources;

use FlowCatalyst\Client\FlowCatalystClient;
use FlowCatalyst\DTOs\Application;
use FlowCatalyst\DTOs\Requests\CreateApplicationRequest;
use FlowCatalyst\DTOs\Requests\UpdateApplicationRequest;
use FlowCatalyst\DTOs\Responses\ApplicationList;

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
