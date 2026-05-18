<?php

declare(strict_types=1);

namespace FlowCatalyst\DTOs;

/**
 * A role scoped to an application — the shape returned by
 * `GET /api/applications/by-id/{id}/roles`. Distinct from the generic
 * `Role` DTO (which carries `name`/`shortName` and timestamps for the
 * full role catalogue endpoint).
 */
final class ApplicationRole
{
    /**
     * @param string[] $permissions
     */
    public function __construct(
        public readonly string $id,
        public readonly string $code,
        public readonly string $displayName,
        public readonly string $applicationCode,
        public readonly string $source,
        public readonly array $permissions,
        public readonly bool $clientManaged,
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
            code: (string) ($data['code'] ?? ''),
            displayName: (string) ($data['displayName'] ?? ''),
            applicationCode: (string) ($data['applicationCode'] ?? ''),
            source: (string) ($data['source'] ?? ''),
            permissions: $permissions,
            clientManaged: (bool) ($data['clientManaged'] ?? false),
            description: isset($data['description']) ? (string) $data['description'] : null,
        );
    }
}
