<?php

declare(strict_types=1);

namespace FlowCatalyst\DTOs\Responses;

use FlowCatalyst\DTOs\Client;

/**
 * Paginated list of clients returned by GET /api/clients and /search.
 */
final class ClientList
{
    /**
     * @param Client[] $clients
     */
    public function __construct(
        public readonly array $clients,
        public readonly int $total,
    ) {}

    /**
     * @param array<string, mixed> $data
     */
    public static function fromArray(array $data): self
    {
        /** @var array<int, array<string, mixed>> $rows */
        $rows = $data['clients'] ?? [];
        return new self(
            clients: array_map(
                fn(array $row) => Client::fromArray($row),
                $rows,
            ),
            total: (int) ($data['total'] ?? 0),
        );
    }
}
