<?php

declare(strict_types=1);

namespace FlowCatalyst\DTOs\Responses;

use FlowCatalyst\DTOs\RoleAssignment;

/**
 * Result of PUT /api/principals/{id}/roles (declarative batch assign).
 *
 * `roles` is the full set of role assignments after the update.
 * `added` / `removed` describe the diff applied by this call.
 */
final class BatchAssignRolesResult
{
    /**
     * @param RoleAssignment[] $roles
     * @param string[] $added
     * @param string[] $removed
     */
    public function __construct(
        public readonly array $roles,
        public readonly array $added,
        public readonly array $removed,
    ) {}

    /**
     * @param array<string, mixed> $data
     */
    public static function fromArray(array $data): self
    {
        /** @var array<int, array<string, mixed>> $rows */
        $rows = $data['roles'] ?? [];
        /** @var string[] $added */
        $added = $data['added'] ?? [];
        /** @var string[] $removed */
        $removed = $data['removed'] ?? [];

        return new self(
            roles: array_map(
                fn(array $row) => RoleAssignment::fromArray($row),
                $rows,
            ),
            added: $added,
            removed: $removed,
        );
    }
}
