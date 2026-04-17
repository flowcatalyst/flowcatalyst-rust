<?php

declare(strict_types=1);

namespace FlowCatalyst\DTOs\Responses;

use FlowCatalyst\DTOs\Connection;

/**
 * Paginated list of connections returned by GET /api/connections.
 */
final class ConnectionList
{
    /**
     * @param Connection[] $connections
     */
    public function __construct(
        public readonly array $connections,
        public readonly int $total,
    ) {}

    /**
     * @param array<string, mixed> $data
     */
    public static function fromArray(array $data): self
    {
        /** @var array<int, array<string, mixed>> $rows */
        $rows = $data['connections'] ?? [];
        return new self(
            connections: array_map(
                fn(array $row) => Connection::fromArray($row),
                $rows,
            ),
            total: (int) ($data['total'] ?? count($rows)),
        );
    }
}
