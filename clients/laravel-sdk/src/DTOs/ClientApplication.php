<?php

declare(strict_types=1);

namespace FlowCatalyst\DTOs;

/**
 * An application as seen from a client's perspective.
 *
 * `active` is whether the application itself is active globally.
 * `enabledForClient` is whether it is enabled specifically for this client.
 * An application can be globally active but disabled for a given client.
 *
 * Returned by GET /api/clients/{id}/applications (inside `applications`).
 */
final class ClientApplication
{
    public function __construct(
        public readonly string $id,
        public readonly string $code,
        public readonly string $name,
        public readonly bool $active,
        public readonly bool $enabledForClient,
        public readonly ?string $description = null,
        public readonly ?string $iconUrl = null,
    ) {}

    /**
     * @param array<string, mixed> $data
     */
    public static function fromArray(array $data): self
    {
        return new self(
            id: (string) $data['id'],
            code: (string) $data['code'],
            name: (string) $data['name'],
            active: (bool) ($data['active'] ?? true),
            enabledForClient: (bool) ($data['enabledForClient'] ?? false),
            description: isset($data['description']) ? (string) $data['description'] : null,
            iconUrl: isset($data['iconUrl']) ? (string) $data['iconUrl'] : null,
        );
    }

    /**
     * @return array<string, mixed>
     */
    public function toArray(): array
    {
        return [
            'id' => $this->id,
            'code' => $this->code,
            'name' => $this->name,
            'active' => $this->active,
            'enabledForClient' => $this->enabledForClient,
            'description' => $this->description,
            'iconUrl' => $this->iconUrl,
        ];
    }
}
