<?php

declare(strict_types=1);

namespace FlowCatalyst\DTOs\Responses;

use FlowCatalyst\DTOs\Permission;

/**
 * Permissions catalogue returned by GET /api/roles/permissions.
 */
final class PermissionList
{
    /**
     * @param Permission[] $permissions
     */
    public function __construct(
        public readonly array $permissions,
        public readonly int $total,
    ) {}

    /**
     * @param array<string, mixed> $data
     */
    public static function fromArray(array $data): self
    {
        /** @var array<int, array<string, mixed>> $rows */
        $rows = $data['permissions'] ?? [];
        return new self(
            permissions: array_map(
                fn(array $row) => Permission::fromArray($row),
                $rows,
            ),
            total: (int) ($data['total'] ?? count($rows)),
        );
    }
}
