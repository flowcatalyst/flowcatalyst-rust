<?php

declare(strict_types=1);

namespace FlowCatalyst\DTOs;

final class Application
{
    public function __construct(
        public readonly string $id,
        public readonly string $code,
        public readonly string $name,
        public readonly string $type,
        public readonly ?string $description,
        public readonly ?string $defaultBaseUrl,
        public readonly ?string $iconUrl,
        public readonly ?string $serviceAccountId,
        public readonly bool $active,
        public readonly string $createdAt,
        public readonly string $updatedAt,
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
            type: (string) ($data['type'] ?? 'APPLICATION'),
            description: isset($data['description']) ? (string) $data['description'] : null,
            defaultBaseUrl: isset($data['defaultBaseUrl']) ? (string) $data['defaultBaseUrl'] : null,
            iconUrl: isset($data['iconUrl']) ? (string) $data['iconUrl'] : null,
            serviceAccountId: isset($data['serviceAccountId']) ? (string) $data['serviceAccountId'] : null,
            active: (bool) ($data['active'] ?? true),
            createdAt: (string) ($data['createdAt'] ?? ''),
            updatedAt: (string) ($data['updatedAt'] ?? ''),
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
            'type' => $this->type,
            'description' => $this->description,
            'defaultBaseUrl' => $this->defaultBaseUrl,
            'iconUrl' => $this->iconUrl,
            'serviceAccountId' => $this->serviceAccountId,
            'active' => $this->active,
            'createdAt' => $this->createdAt,
            'updatedAt' => $this->updatedAt,
        ];
    }
}
