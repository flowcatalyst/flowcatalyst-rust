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
     * List audit logs with optional filters and pagination.
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
    ): AuditLogList {
        $params = array_filter(
            [
                'entityType' => $entityType,
                'entityId' => $entityId,
                'operation' => $operation,
                'principalId' => $principalId,
                'clientId' => $clientId,
                'from' => $from,
                'to' => $to,
                'page' => $page,
                'pageSize' => $pageSize,
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
