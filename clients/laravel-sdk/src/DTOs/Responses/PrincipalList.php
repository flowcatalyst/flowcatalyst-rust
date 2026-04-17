<?php

declare(strict_types=1);

namespace FlowCatalyst\DTOs\Responses;

use FlowCatalyst\DTOs\Principal;

/**
 * Paginated list of principals returned by GET /api/principals.
 */
final class PrincipalList
{
    /**
     * @param Principal[] $principals
     */
    public function __construct(
        public readonly array $principals,
        public readonly int $total,
    ) {}

    /**
     * @param array<string, mixed> $data
     */
    public static function fromArray(array $data): self
    {
        /** @var array<int, array<string, mixed>> $rows */
        $rows = $data['principals'] ?? [];
        return new self(
            principals: array_map(
                fn(array $row) => Principal::fromArray($row),
                $rows,
            ),
            total: (int) ($data['total'] ?? 0),
        );
    }
}
