<?php

declare(strict_types=1);

namespace FlowCatalyst\DTOs;

/**
 * A role assigned to a principal.
 *
 * Matches the platform's RoleAssignmentDto returned by
 * GET /api/principals/{id}/roles and as items inside
 * PUT /api/principals/{id}/roles response.
 */
final class RoleAssignment
{
    public function __construct(
        public readonly string $id,
        public readonly string $roleName,
        public readonly string $assignmentSource,
        public readonly string $assignedAt,
    ) {}

    /**
     * @param array<string, mixed> $data
     */
    public static function fromArray(array $data): self
    {
        return new self(
            id: (string) $data['id'],
            roleName: (string) $data['roleName'],
            assignmentSource: (string) $data['assignmentSource'],
            assignedAt: (string) $data['assignedAt'],
        );
    }

    /**
     * @return array<string, string>
     */
    public function toArray(): array
    {
        return [
            'id' => $this->id,
            'roleName' => $this->roleName,
            'assignmentSource' => $this->assignmentSource,
            'assignedAt' => $this->assignedAt,
        ];
    }
}
