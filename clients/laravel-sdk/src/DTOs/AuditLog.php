<?php

declare(strict_types=1);

namespace FlowCatalyst\DTOs;

/**
 * A row from the platform's `iam_audit_logs` table — one entry per UoW
 * commit (in addition to the corresponding domain event in `msg_events`).
 */
final class AuditLog
{
    public function __construct(
        public readonly string $id,
        public readonly string $operation,
        public readonly string $entityType,
        public readonly string $performedAt,
        public readonly ?string $entityId = null,
        public readonly ?string $principalId = null,
        public readonly ?string $principalName = null,
        public readonly ?string $applicationId = null,
        public readonly ?string $clientId = null,
    ) {}

    /**
     * @param array<string, mixed> $data
     */
    public static function fromArray(array $data): self
    {
        return new self(
            id: (string) $data['id'],
            operation: (string) $data['operation'],
            entityType: (string) $data['entityType'],
            performedAt: (string) ($data['performedAt'] ?? ''),
            entityId: isset($data['entityId']) ? (string) $data['entityId'] : null,
            principalId: isset($data['principalId']) ? (string) $data['principalId'] : null,
            principalName: isset($data['principalName']) ? (string) $data['principalName'] : null,
            applicationId: isset($data['applicationId']) ? (string) $data['applicationId'] : null,
            clientId: isset($data['clientId']) ? (string) $data['clientId'] : null,
        );
    }
}
