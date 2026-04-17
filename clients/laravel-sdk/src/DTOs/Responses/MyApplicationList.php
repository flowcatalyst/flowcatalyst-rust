<?php

declare(strict_types=1);

namespace FlowCatalyst\DTOs\Responses;

use FlowCatalyst\DTOs\MyApplication;

/**
 * Applications enabled for a client the user has access to, returned
 * by GET /api/me/clients/{clientId}/applications.
 */
final class MyApplicationList
{
    /**
     * @param MyApplication[] $applications
     */
    public function __construct(
        public readonly array $applications,
        public readonly int $total,
        public readonly string $clientId,
    ) {}

    /**
     * @param array<string, mixed> $data
     */
    public static function fromArray(array $data): self
    {
        /** @var array<int, array<string, mixed>> $rows */
        $rows = $data['applications'] ?? [];
        return new self(
            applications: array_map(
                fn(array $row) => MyApplication::fromArray($row),
                $rows,
            ),
            total: (int) ($data['total'] ?? count($rows)),
            clientId: (string) ($data['clientId'] ?? ''),
        );
    }
}
