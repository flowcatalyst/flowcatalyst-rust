<?php

declare(strict_types=1);

namespace FlowCatalyst\DTOs\Requests;

/**
 * Payload for POST /api/roles.
 *
 * `roleName` is the short name (e.g. `admin`); the platform prefixes
 * it with `applicationCode` to produce the full name (`orders:admin`).
 */
final class CreateRoleRequest
{
    /**
     * @param string[] $permissions
     */
    public function __construct(
        public readonly string $applicationCode,
        public readonly string $roleName,
        public readonly string $displayName,
        public readonly ?string $description = null,
        public readonly array $permissions = [],
        public readonly bool $clientManaged = false,
    ) {}

    /**
     * @return array<string, mixed>
     */
    public function toArray(): array
    {
        $payload = [
            'applicationCode' => $this->applicationCode,
            'roleName' => $this->roleName,
            'displayName' => $this->displayName,
            'permissions' => $this->permissions,
            'clientManaged' => $this->clientManaged,
        ];
        if ($this->description !== null) {
            $payload['description'] = $this->description;
        }
        return $payload;
    }
}
