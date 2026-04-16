<?php

declare(strict_types=1);

namespace FlowCatalyst\Client\Resources;

use FlowCatalyst\Client\FlowCatalystClient;
use FlowCatalyst\DTOs\Principal;
use FlowCatalyst\Exceptions\FlowCatalystException;

class Principals
{
    public function __construct(
        private readonly FlowCatalystClient $client
    ) {}

    /**
     * List all principals with optional filters.
     *
     * @return array{principals: Principal[], total: int}
     */
    public function list(
        ?string $clientId = null,
        ?string $type = null,
        ?bool $active = null,
        ?string $email = null
    ): array {
        $queryParams = [];

        if ($clientId !== null) {
            $queryParams['clientId'] = $clientId;
        }
        if ($type !== null) {
            $queryParams['type'] = $type;
        }
        if ($active !== null) {
            $queryParams['active'] = $active ? 'true' : 'false';
        }
        if ($email !== null) {
            $queryParams['email'] = $email;
        }

        $query = !empty($queryParams) ? '?' . http_build_query($queryParams) : '';

        $response = $this->client->request('GET', "/api/principals{$query}");

        return [
            'principals' => array_map(
                fn(array $item) => Principal::fromArray($item),
                $response['principals'] ?? []
            ),
            'total' => $response['total'] ?? 0,
        ];
    }

    /**
     * Get a principal by ID.
     */
    public function get(string $id): Principal
    {
        $response = $this->client->request('GET', "/api/principals/{$id}");

        return Principal::fromArray($response);
    }

    /**
     * Find a principal by email address.
     *
     * Returns null if no principal exists with the given email.
     */
    public function findByEmail(string $email): ?Principal
    {
        $result = $this->list(email: $email);

        if ($result['total'] === 0) {
            return null;
        }

        return $result['principals'][0] ?? null;
    }

    /**
     * Create a new user principal.
     *
     * `enforcePasswordComplexity` defaults to true. Pass false when your app
     * enforces its own password policy and only the platform's 2-character
     * minimum should apply.
     *
     * @param array{
     *     email: string,
     *     name: string,
     *     password?: string,
     *     clientId?: string,
     *     enforcePasswordComplexity?: bool
     * } $data
     */
    public function createUser(array $data): Principal
    {
        $response = $this->client->request('POST', '/api/principals/users', [
            'json' => $data,
        ]);

        return Principal::fromArray($response);
    }

    /**
     * Reset a user's password (admin operation).
     *
     * Uses the admin endpoint; requires admin credentials on the underlying
     * client. Pass `$enforcePasswordComplexity = false` when the caller enforces
     * its own password policy.
     */
    public function resetPassword(
        string $id,
        string $newPassword,
        bool $enforcePasswordComplexity = true
    ): void {
        $this->client->request('POST', "/api/principals/{$id}/reset-password", [
            'json' => [
                'newPassword' => $newPassword,
                'enforcePasswordComplexity' => $enforcePasswordComplexity,
            ],
        ]);
    }

    /**
     * Update a principal's details.
     *
     * @param array{
     *     name?: string
     * } $data
     */
    public function update(string $id, array $data): Principal
    {
        $response = $this->client->request('PUT', "/api/principals/{$id}", [
            'json' => $data,
        ]);

        return Principal::fromArray($response);
    }

    /**
     * Activate a principal.
     */
    public function activate(string $id): void
    {
        $this->client->request('POST', "/api/principals/{$id}/activate");
    }

    /**
     * Deactivate a principal.
     */
    public function deactivate(string $id): void
    {
        $this->client->request('POST', "/api/principals/{$id}/deactivate");
    }

    /**
     * Get roles assigned to a principal.
     *
     * @return array{roles: array<array{roleName: string, assignmentSource: string, assignedAt: string}>}
     */
    public function getRoles(string $id): array
    {
        return $this->client->request('GET', "/api/principals/{$id}/roles");
    }

    /**
     * Add a single role to a principal.
     *
     * @return array{roleName: string, assignmentSource: string, assignedAt: string}
     */
    public function assignRole(string $id, string $roleName): array
    {
        return $this->client->request('POST', "/api/principals/{$id}/roles/{$roleName}");
    }

    /**
     * Remove a role from a principal.
     */
    public function removeRole(string $id, string $roleName): void
    {
        $this->client->request('DELETE', "/api/principals/{$id}/roles/{$roleName}");
    }

    /**
     * Batch assign roles to a principal (declarative).
     *
     * This sets the complete list of roles for the principal.
     * Roles not in the list will be removed.
     *
     * @param string[] $roles Role names to assign
     * @return array{roles: array, added: string[], removed: string[]}
     */
    public function assignRoles(string $id, array $roles): array
    {
        return $this->client->request('PUT', "/api/principals/{$id}/roles", [
            'json' => ['roles' => $roles],
        ]);
    }

    /**
     * Get client access grants for a principal.
     *
     * @return array{grants: array<array{id: string, clientId: string, grantedAt: string, expiresAt: ?string}>}
     */
    public function getClientAccessGrants(string $id): array
    {
        return $this->client->request('GET', "/api/principals/{$id}/clients");
    }

    /**
     * Grant client access to a principal.
     *
     * @return array{id: string, clientId: string, grantedAt: string, expiresAt: ?string}
     */
    public function grantClientAccess(string $id, string $clientId): array
    {
        return $this->client->request('POST', "/api/principals/{$id}/clients/{$clientId}");
    }

    /**
     * Revoke client access from a principal.
     */
    public function revokeClientAccess(string $id, string $clientId): void
    {
        $this->client->request('DELETE', "/api/principals/{$id}/clients/{$clientId}");
    }

    /**
     * Sync principals for an application.
     *
     * This creates/updates user principals and assigns roles (prefixed with
     * the application code). If removeUnlisted is true, SDK-synced roles
     * for unlisted principals will be removed.
     *
     * @param string $appCode The application code
     * @param array<array{
     *     email: string,
     *     name: string,
     *     roles?: string[],
     *     active?: bool
     * }> $principals The principals to sync
     * @param bool $removeUnlisted If true, removes SDK-synced roles for unlisted principals
     * @return array{applicationCode: string, created: int, updated: int, deleted: int, syncedCodes: string[]}
     */
    public function sync(string $appCode, array $principals, bool $removeUnlisted = false): array
    {
        $query = $removeUnlisted ? '?removeUnlisted=true' : '';

        return $this->client->request('POST', "/api/applications/{$appCode}/principals/sync{$query}", [
            'json' => ['principals' => $principals],
        ]);
    }
}
