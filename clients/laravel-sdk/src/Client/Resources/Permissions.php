<?php

declare(strict_types=1);

namespace FlowCatalyst\Client\Resources;

use FlowCatalyst\Client\FlowCatalystClient;
use FlowCatalyst\DTOs\Permission;

class Permissions
{
    public function __construct(
        private readonly FlowCatalystClient $client
    ) {}

    /**
     * List all permissions.
     *
     * @return array{permissions: Permission[], total: int}
     */
    public function list(): array
    {
        $response = $this->client->request('GET', '/api/roles/permissions');

        return [
            'permissions' => array_map(
                fn(array $item) => Permission::fromArray($item),
                $response['permissions'] ?? []
            ),
            'total' => $response['total'] ?? count($response['permissions'] ?? []),
        ];
    }

    /**
     * Get a permission by name.
     */
    public function get(string $permission): Permission
    {
        $response = $this->client->request('GET', "/api/roles/permissions/{$permission}");

        return Permission::fromArray($response);
    }
}
