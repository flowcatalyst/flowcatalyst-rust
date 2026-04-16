<?php

declare(strict_types=1);

namespace FlowCatalyst\Client\Resources;

use FlowCatalyst\Client\FlowCatalystClient;
use FlowCatalyst\DTOs\Role;

class Roles
{
    public function __construct(
        private readonly FlowCatalystClient $client
    ) {}

    /**
     * List all roles.
     *
     * @return array{roles: Role[], total: int}
     */
    public function list(): array
    {
        $response = $this->client->request('GET', '/api/roles');

        return [
            'roles' => array_map(
                fn(array $item) => Role::fromArray($item),
                $response['roles'] ?? []
            ),
            'total' => $response['total'] ?? count($response['roles'] ?? []),
        ];
    }

    /**
     * Get a role by name.
     */
    public function get(string $roleName): Role
    {
        $response = $this->client->request('GET', "/api/roles/{$roleName}");

        return Role::fromArray($response);
    }

    /**
     * Create a new role.
     *
     * @param array{
     *     applicationCode?: string,
     *     name: string,
     *     displayName?: string,
     *     description?: string,
     *     permissions?: string[],
     *     clientManaged?: bool
     * } $data
     */
    public function create(array $data): Role
    {
        $response = $this->client->request('POST', '/api/roles', [
            'json' => $data,
        ]);

        return Role::fromArray($response);
    }

    /**
     * Update a role.
     *
     * @param array{
     *     displayName?: string,
     *     description?: string,
     *     permissions?: string[]
     * } $data
     */
    public function update(string $roleName, array $data): Role
    {
        $response = $this->client->request('PUT', "/api/roles/{$roleName}", [
            'json' => $data,
        ]);

        return Role::fromArray($response);
    }

    /**
     * Delete a role.
     */
    public function delete(string $roleName): void
    {
        $this->client->request('DELETE', "/api/roles/{$roleName}");
    }

    /**
     * Sync roles for an application.
     *
     * This creates/updates roles with source=SDK and optionally removes
     * SDK roles not in the sync list.
     *
     * @param string $appCode The application code
     * @param array<array{
     *     name: string,
     *     displayName?: string,
     *     description?: string,
     *     permissions?: string[],
     *     clientManaged?: bool
     * }> $roles The roles to sync
     * @param bool $removeUnlisted If true, removes SDK roles not in the list
     * @return array{syncedCount: int, roles: Role[]}
     */
    public function sync(string $appCode, array $roles, bool $removeUnlisted = false): array
    {
        $query = $removeUnlisted ? '?removeUnlisted=true' : '';

        $response = $this->client->request('POST', "/api/applications/{$appCode}/roles/sync{$query}", [
            'json' => ['roles' => $roles],
        ]);

        return [
            'syncedCount' => $response['syncedCount'] ?? count($response['roles'] ?? []),
            'roles' => array_map(
                fn(array $item) => Role::fromArray($item),
                $response['roles'] ?? []
            ),
        ];
    }
}
