<?php

declare(strict_types=1);

namespace FlowCatalyst\DTOs\Responses;

use FlowCatalyst\DTOs\AuditLog;

/**
 * Wraps `GET /api/audit-logs` — paginated list of audit log entries.
 */
final class AuditLogList
{
    /**
     * @param AuditLog[] $auditLogs
     */
    public function __construct(
        public readonly array $auditLogs,
        public readonly int $total = 0,
        public readonly int $page = 0,
        public readonly int $pageSize = 0,
    ) {}

    /**
     * @param array<string, mixed> $data
     */
    public static function fromArray(array $data): self
    {
        /** @var array<int, array<string, mixed>> $rows */
        $rows = $data['auditLogs'] ?? [];
        return new self(
            auditLogs: array_map(static fn(array $row) => AuditLog::fromArray($row), $rows),
            total: (int) ($data['total'] ?? 0),
            page: (int) ($data['page'] ?? 0),
            pageSize: (int) ($data['pageSize'] ?? 0),
        );
    }
}
