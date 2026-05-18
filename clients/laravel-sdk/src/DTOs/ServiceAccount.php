<?php

declare(strict_types=1);

namespace FlowCatalyst\DTOs;

/**
 * A service-account principal — the non-human credential attached to an
 * application. Provisioned via Applications::provisionServiceAccount.
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
        ];
    }
}
