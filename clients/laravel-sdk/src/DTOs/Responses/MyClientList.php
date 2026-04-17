<?php

declare(strict_types=1);

namespace FlowCatalyst\DTOs\Responses;

use FlowCatalyst\DTOs\MyClient;

/**
 * List of clients the authenticated user can access, returned by
 * GET /api/me/clients.
 */
final class MyClientList
{
    /**
     * @param MyClient[] $clients
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
                fn(array $row) => MyClient::fromArray($row),
                $rows,
            ),
            total: (int) ($data['total'] ?? count($rows)),
        );
    }
}
