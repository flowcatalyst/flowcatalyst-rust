<?php

declare(strict_types=1);

namespace FlowCatalyst\Client\Resources;

use FlowCatalyst\Client\FlowCatalystClient;
use FlowCatalyst\DTOs\AuditLog;
use FlowCatalyst\DTOs\Responses\AuditLogList;

/**
 * Read-only queries against the platform's audit-log table.
 */
class AuditLogs
{
    public function __construct(
        private readonly FlowCatalystClient $client
    ) {}

    /**
     * List audit logs, newest first, with cursor paging.
     *
     * The platform pages with a cursor: pass the previous page's
     * `$result->nextCursor` as `$after` while `$result->hasMore` is true.
     * `$pageSize` sets the page size. `$clientId` is sent as a one-element
     * `clientIds` filter; `$clientIds` and `$applicationIds` filter on any of
     * the listed ids.
     *
     * `$from`, `$to` and `$page` are deprecated: the platform has no date or
     * page-number filters and ignores them. They are still sent, for older
     * platforms.
     *
     * @param string[]|null $applicationIds
     * @param string[]|null $clientIds
     */
    public function list(
        ?string $entityType = null,
        ?string $entityId = null,
        ?string $operation = null,
        ?string $principalId = null,
        ?string $clientId = null,
        ?string $from = null,
        ?string $to = null,
        ?int $page = null,
        ?int $pageSize = null,
        ?string $after = null,
        ?array $applicationIds = null,
        ?array $clientIds = null,
    ): AuditLogList {
        $clientIdFilter = $clientIds ?? [];
        if ($clientId !== null && !in_array($clientId, $clientIdFilter, true)) {
            $clientIdFilter[] = $clientId;
        }

        $params = array_filter(
            [
                'after' => $after,
                'pageSize' => $pageSize,
                'entityType' => $entityType,
                'entityId' => $entityId,
                'operation' => $operation,
                'principalId' => $principalId,
                'applicationIds' => $applicationIds !== null && $applicationIds !== []
                    ? implode(',', $applicationIds)
                    : null,
                'clientIds' => $clientIdFilter !== [] ? implode(',', $clientIdFilter) : null,
                'from' => $from,
                'to' => $to,
                'page' => $page,
            ],
            static fn($v) => $v !== null,
        );

        $query = $params !== [] ? '?' . http_build_query($params) : '';

        $response = $this->client->request('GET', "/api/audit-logs{$query}");

        return AuditLogList::fromArray($response);
    }

    /**
     * Get a single audit log entry by ID.
     */
    public function get(string $id): AuditLog
    {
        $response = $this->client->request('GET', "/api/audit-logs/{$id}");

        return AuditLog::fromArray($response);
    }
}
