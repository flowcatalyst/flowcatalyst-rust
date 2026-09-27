<?php

declare(strict_types=1);

namespace FlowCatalyst\DTOs;

/**
 * A service-account principal — the non-human credential attached to an
 * application, as returned by `GET /api/service-accounts/{id}` (and so by
 * Applications::getServiceAccount). Provisioning returns a different shape:
 * see Applications::provisionServiceAccount.
 */
final class ServiceAccount
{
    public function __construct(
        public readonly string $id,
        public readonly string $code,
        public readonly string $name,
        public readonly bool $active,
        public readonly string $createdAt,
        public readonly ?string $description = null,
        public readonly ?string $applicationId = null,
        public readonly ?string $principalId = null,
        public readonly ?string $oauthClientId = null,
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
            createdAt: (string) ($data['createdAt'] ?? ''),
            description: isset($data['description']) ? (string) $data['description'] : null,
            applicationId: isset($data['applicationId']) ? (string) $data['applicationId'] : null,
            principalId: isset($data['principalId']) ? (string) $data['principalId'] : null,
            oauthClientId: isset($data['oauthClientId']) ? (string) $data['oauthClientId'] : null,
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
            'description' => $this->description,
            'active' => $this->active,
            'applicationId' => $this->applicationId,
            'createdAt' => $this->createdAt,
            'principalId' => $this->principalId,
            'oauthClientId' => $this->oauthClientId,
        ];
    }
}
