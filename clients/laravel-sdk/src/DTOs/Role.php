<?php

declare(strict_types=1);

namespace FlowCatalyst\DTOs;

/**
 * A role assignable to principals.
 *
 * `name` is the full role code (prefixed with application, e.g. `orders:admin`).
 * `shortName` is the segment after the colon (e.g. `admin`).
 */
final class Role
{
    /**
     * @param string[] $permissions
     */
    public function __construct(
        public readonly string $id,
        public readonly string $name,
        public readonly string $shortName,
        public readonly string $displayName,
        public readonly string $applicationCode,
        public readonly string $source,
        public readonly array $permissions,
        public readonly bool $clientManaged,
        public readonly string $createdAt,
        public readonly string $updatedAt,
        public readonly ?string $description = null,
    ) {}

    /**
     * @param array<string, mixed> $data
     */
    public static function fromArray(array $data): self
    {
        /** @var string[] $permissions */
        $permissions = $data['permissions'] ?? [];
        return new self(
            id: (string) ($data['id'] ?? ''),
            name: (string) $data['name'],
            shortName: (string) ($data['shortName'] ?? ''),
            displayName: (string) ($data['displayName'] ?? ''),
            applicationCode: (string) ($data['applicationCode'] ?? ''),
            source: (string) ($data['source'] ?? ''),
            permissions: $permissions,
            clientManaged: (bool) ($data['clientManaged'] ?? false),
            createdAt: (string) ($data['createdAt'] ?? ''),
            updatedAt: (string) ($data['updatedAt'] ?? ''),
            description: isset($data['description']) ? (string) $data['description'] : null,
        );
    }

    /**
     * @return array<string, mixed>
     */
    public function toArray(): array
    {
        return [
            'id' => $this->id,
            'name' => $this->name,
            'shortName' => $this->shortName,
            'displayName' => $this->displayName,
            'applicationCode' => $this->applicationCode,
            'source' => $this->source,
            'permissions' => $this->permissions,
            'clientManaged' => $this->clientManaged,
            'createdAt' => $this->createdAt,
            'updatedAt' => $this->updatedAt,
            'description' => $this->description,
        ];
    }

    /**
     * Check if this role is SDK-managed.
     */
    public function isSdkManaged(): bool
    {
        return $this->source === 'SDK';
    }

    /**
     * Check if this role is from code (hardcoded).
     */
    public function isFromCode(): bool
    {
        return $this->source === 'CODE';
    }
}
