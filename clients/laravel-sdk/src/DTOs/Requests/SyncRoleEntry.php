<?php

declare(strict_types=1);

namespace FlowCatalyst\DTOs\Requests;

/**
 * One entry in the POST /api/applications/{appCode}/roles/sync payload.
 */
final class SyncRoleEntry
{
    /**
     * @param string[] $permissions
     */
    public function __construct(
        public readonly string $name,
        public readonly ?string $displayName = null,
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
            'name' => $this->name,
            'permissions' => $this->permissions,
            'clientManaged' => $this->clientManaged,
        ];
        if ($this->displayName !== null) {
            $payload['displayName'] = $this->displayName;
        }
        if ($this->description !== null) {
            $payload['description'] = $this->description;
        }
        return $payload;
    }
}
