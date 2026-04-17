<?php

declare(strict_types=1);

namespace FlowCatalyst\DTOs\Requests;

/**
 * Payload for PUT /api/roles/{roleName}. Only provided fields are updated.
 *
 * Permissions are not edited here — use `grantPermission` / `revokePermission`
 * (or the batch sync endpoint) to change the permission set.
 */
final class UpdateRoleRequest
{
    public function __construct(
        public readonly ?string $displayName = null,
        public readonly ?string $description = null,
        public readonly ?bool $clientManaged = null,
    ) {}

    /**
     * @return array<string, mixed>
     */
    public function toArray(): array
    {
        $payload = [];
        if ($this->displayName !== null) {
            $payload['displayName'] = $this->displayName;
        }
        if ($this->description !== null) {
            $payload['description'] = $this->description;
        }
        if ($this->clientManaged !== null) {
            $payload['clientManaged'] = $this->clientManaged;
        }
        return $payload;
    }
}
