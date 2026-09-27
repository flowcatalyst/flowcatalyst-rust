<?php

declare(strict_types=1);

namespace FlowCatalyst\DTOs\Responses;

use FlowCatalyst\DTOs\AuditLog;

/**
 * Wraps `GET /api/audit-logs` — a page of audit log entries.
 *
 * The platform pages with a cursor: while `$hasMore` is true, pass
 * `$nextCursor` as `after` to fetch the next page. `$total`, `$page` and
 * `$pageSize` are only filled by older platforms that paged by number.
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
        public readonly bool $hasMore = false,
        public readonly ?string $nextCursor = null,
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
            hasMore: (bool) ($data['hasMore'] ?? false),
            nextCursor: isset($data['nextCursor']) && $data['nextCursor'] !== ''
                ? (string) $data['nextCursor']
                : null,
        );
    }
}
