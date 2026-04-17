<?php

declare(strict_types=1);

namespace FlowCatalyst\Client\Resources;

use FlowCatalyst\Client\FlowCatalystClient;
use FlowCatalyst\DTOs\Requests\CreateRoleRequest;
use FlowCatalyst\DTOs\Requests\SyncRoleEntry;
use FlowCatalyst\DTOs\Requests\UpdateRoleRequest;
use FlowCatalyst\DTOs\Responses\RoleList;
use FlowCatalyst\DTOs\Responses\SyncResult;
use FlowCatalyst\DTOs\Role;

class Roles
{
    public function __construct(
        private readonly FlowCatalystClient $client
    ) {}

    /**
     * List roles.
     */
    public function list(
        ?string $applicationCode = null,
        ?string $source = null,
        ?bool $clientManaged = null,
    ): RoleList {
        $queryParams = [];
        if ($applicationCode !== null) {
            $queryParams['applicationCode'] = $applicationCode;
        }
        if ($source !== null) {
            $queryParams['source'] = $source;
        }
        if ($clientManaged !== null) {
            $queryParams['clientManaged'] = $clientManaged ? 'true' : 'false';
        }
        $query = !empty($queryParams) ? '?' . http_build_query($queryParams) : '';
        $response = $this->client->request('GET', "/api/roles{$query}");

        return RoleList::fromArray($response);
    }

    /**
     * Get a role by name or ID.
     */
    public function get(string $roleName): Role
    {
        $response = $this->client->request('GET', "/api/roles/{$roleName}");

        return Role::fromArray($response);
    }

    /**
     * Get a role by its full code.
     */
    public function getByCode(string $code): Role
    {
        $response = $this->client->request('GET', "/api/roles/by-code/{$code}");

        return Role::fromArray($response);
    }

    /**
     * Create a new role.
     *
     * Returns the created role's ID. Call `get($id)` if you need the
     * full record.
     */
    public function create(CreateRoleRequest $request): string
    {
        $response = $this->client->request('POST', '/api/roles', [
            'json' => $request->toArray(),
        ]);

        return (string) $response['id'];
    }

    /**
     * Update a role. The platform responds with 204 No Content.
     */
    public function update(string $roleName, UpdateRoleRequest $request): void
    {
        $this->client->request('PUT', "/api/roles/{$roleName}", [
            'json' => $request->toArray(),
        ]);
    }

    /**
     * Grant a permission to a role.
     */
    public function grantPermission(string $roleName, string $permission): Role
    {
        $response = $this->client->request('POST', "/api/roles/{$roleName}/permissions", [
            'json' => ['permission' => $permission],
        ]);

        return Role::fromArray($response);
    }

    /**
     * Revoke a permission from a role.
     */
    public function revokePermission(string $roleName, string $permission): Role
    {
        $response = $this->client->request('DELETE', "/api/roles/{$roleName}/permissions/{$permission}");

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
     * Sync roles for an application. Creates/updates roles with
     * source=`SDK` and, when `$removeUnlisted` is true, removes
     * SDK-managed roles not in the sync list.
     *
     * @param SyncRoleEntry[] $roles
     */
    public function sync(
        string $applicationCode,
        array $roles,
        bool $removeUnlisted = false,
    ): SyncResult {
        $query = $removeUnlisted ? '?removeUnlisted=true' : '';

        $response = $this->client->request(
            'POST',
            "/api/applications/{$applicationCode}/roles/sync{$query}",
            [
                'json' => [
                    'roles' => array_map(
                        fn(SyncRoleEntry $entry) => $entry->toArray(),
                        $roles,
                    ),
                ],
            ],
        );

        return SyncResult::fromArray($response);
    }
}
