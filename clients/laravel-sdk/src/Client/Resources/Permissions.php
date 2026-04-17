<?php

declare(strict_types=1);

namespace FlowCatalyst\Client\Resources;

use FlowCatalyst\Client\FlowCatalystClient;
use FlowCatalyst\DTOs\Permission;
use FlowCatalyst\DTOs\Responses\PermissionList;

class Permissions
{
    public function __construct(
        private readonly FlowCatalystClient $client
    ) {}

    /**
     * List the platform's permission catalogue.
     */
    public function list(): PermissionList
    {
        $response = $this->client->request('GET', '/api/roles/permissions');

        return PermissionList::fromArray($response);
    }

    /**
     * Get a single permission by its full string.
     */
    public function get(string $permission): Permission
    {
        $response = $this->client->request('GET', "/api/roles/permissions/{$permission}");

        return Permission::fromArray($response);
    }
}
