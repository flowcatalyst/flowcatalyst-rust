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
use FlowCatalyst\DTOs\Responses\ProvisionServiceAccountResult;
use FlowCatalyst\DTOs\ServiceAccount;
use FlowCatalyst\Exceptions\FlowCatalystException;

class Applications
{
    public function __construct(
        private readonly FlowCatalystClient $client
    ) {}

    /**
     * List applications, optionally filtered by `active` and `type`
     * (`APPLICATION` or `INTEGRATION`).
     */
    public function list(?bool $active = null, ?string $type = null): ApplicationList
    {
        $params = [];
        if ($active !== null) {
            $params['active'] = $active ? 'true' : 'false';
        }
        if ($type !== null) {
            $params['type'] = $type;
        }
        $query = $params !== [] ? '?' . http_build_query($params) : '';

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
     *
     * Returns the created application's ID. The platform's create endpoint
     * returns `{id}` only; call `get($id)` if you need the full record.
     */
    public function create(CreateApplicationRequest $request): string
    {
        $response = $this->client->request('POST', '/api/applications', [
            'json' => $request->toArray(),
        ]);

        return (string) ($response['id'] ?? '');
    }

    /**
     * Update an application. The platform responds with 204 No Content;
     * call `get($id)` if you need the updated record.
     */
    public function update(string $id, UpdateApplicationRequest $request): void
    {
        $this->client->request('PUT', "/api/applications/{$id}", [
            'json' => $request->toArray(),
        ]);
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
     *
     * The platform returns
     * `{message, serviceAccount: {principalId, name, oauthClient: {id, clientId, clientSecret}}}`.
     * The client secret is in `$result->serviceAccount->oauthClient->clientSecret`
     * and is only returned once.
     */
    public function provisionServiceAccount(string $id): ProvisionServiceAccountResult
    {
        $response = $this->client->request(
            'POST',
            "/api/applications/{$id}/provision-service-account",
        );

        return ProvisionServiceAccountResult::fromArray($response);
    }

    /**
     * Get the service account attached to an application.
     *
     * The platform has no read route for an application's service account, so
     * this reads the application and then
     * `GET /api/service-accounts/{serviceAccountId}`.
     *
     * @throws FlowCatalystException with code 404 when the application has no
     *         service account
     */
    public function getServiceAccount(string $id): ServiceAccount
    {
        $serviceAccountId = $this->get($id)->serviceAccountId;

        if ($serviceAccountId === null || $serviceAccountId === '') {
            throw new FlowCatalystException(
                "Application {$id} has no service account",
                404,
                null,
                ['error' => 'NOT_FOUND', 'applicationId' => $id],
            );
        }

        $response = $this->client->request('GET', "/api/service-accounts/{$serviceAccountId}");

        return ServiceAccount::fromArray($response);
    }

    /**
     * List roles defined for an application (by TSID).
     *
     * The platform returns role names only (`{roles: [string]}`). Each name
     * comes back as an ApplicationRole whose `code` is the role name and whose
     * other fields are empty.
     *
     * @deprecated Use listRoleNames(): the platform returns names, not role records.
     *
     * @return ApplicationRole[]
     */
    public function listRoles(string $id): array
    {
        $response = $this->client->request('GET', "/api/applications/by-id/{$id}/roles");

        if (array_is_list($response)) {
            // Older platforms returned role records.
            /** @var array<int, array<string, mixed>> $response */
            return array_map(static fn(array $row) => ApplicationRole::fromArray($row), $response);
        }

        return array_map(
            static fn(string $name) => new ApplicationRole(
                id: '',
                code: $name,
                displayName: '',
                applicationCode: '',
                source: '',
                permissions: [],
                clientManaged: false,
            ),
            self::roleNames($response),
        );
    }

    /**
     * The names of the roles defined for an application (by TSID):
     * `GET /api/applications/by-id/{id}/roles`, which returns `{roles: [string]}`.
     *
     * @return string[]
     */
    public function listRoleNames(string $id): array
    {
        $response = $this->client->request('GET', "/api/applications/by-id/{$id}/roles");

        if (array_is_list($response)) {
            // Older platforms returned role records.
            return array_values(array_map(
                static fn($row) => is_array($row) ? (string) ($row['code'] ?? $row['name'] ?? '') : (string) $row,
                $response,
            ));
        }

        return self::roleNames($response);
    }

    /**
     * @param array<string, mixed> $response
     * @return string[]
     */
    private static function roleNames(array $response): array
    {
        $roles = $response['roles'] ?? [];

        return is_array($roles) ? array_values(array_map('strval', $roles)) : [];
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
     * Get one client's configuration for an application.
     */
    public function getClientConfig(string $id, string $clientId): ClientConfig
    {
        $response = $this->client->request('GET', "/api/applications/{$id}/clients/{$clientId}");

        return ClientConfig::fromArray($response);
    }

    /**
     * Update per-client config for an application.
     *
     * @deprecated Only the Rust platform serves `PUT /api/applications/{id}/clients/{clientId}`;
     *             the Go platform has no such route. Use enableForClient() /
     *             disableForClient(), and getClientConfig() to read the result.
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
     * Enable an application for a specific client. The platform responds
     * with 204 No Content; call getClientConfig() for the resulting config.
     */
    public function enableForClient(string $id, string $clientId): void
    {
        $this->client->request(
            'POST',
            "/api/applications/{$id}/clients/{$clientId}/enable",
        );
    }

    /**
     * Disable an application for a specific client. The platform responds
     * with 204 No Content; call getClientConfig() for the resulting config.
     */
    public function disableForClient(string $id, string $clientId): void
    {
        $this->client->request(
            'POST',
            "/api/applications/{$id}/clients/{$clientId}/disable",
        );
    }
}
