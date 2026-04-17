<?php

declare(strict_types=1);

namespace FlowCatalyst\DTOs\Responses;

use FlowCatalyst\DTOs\Role;

/**
 * Paginated list of roles returned by GET /api/roles.
 */
final class RoleList
{
    /**
     * @param Role[] $roles
     */
    public function __construct(
        public readonly array $roles,
        public readonly int $total,
    ) {}

    /**
     * @param array<string, mixed> $data
     */
    public static function fromArray(array $data): self
    {
        /** @var array<int, array<string, mixed>> $rows */
        $rows = $data['roles'] ?? [];
        return new self(
            roles: array_map(
                fn(array $row) => Role::fromArray($row),
                $rows,
            ),
            total: (int) ($data['total'] ?? count($rows)),
        );
    }
}
