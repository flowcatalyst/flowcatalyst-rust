<?php

declare(strict_types=1);

namespace FlowCatalyst\DTOs\Responses;

use FlowCatalyst\DTOs\Application;

/**
 * Paginated list of applications returned by GET /api/applications.
 */
final class ApplicationList
{
    /**
     * @param Application[] $applications
     */
    public function __construct(
        public readonly array $applications,
        public readonly int $total,
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
                fn(array $row) => Application::fromArray($row),
                $rows,
            ),
            total: (int) ($data['total'] ?? 0),
        );
    }
}
