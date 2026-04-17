<?php

declare(strict_types=1);

namespace FlowCatalyst\DTOs\Responses;

use FlowCatalyst\DTOs\EventType;

/**
 * List of event types returned by GET /api/event-types.
 *
 * The platform returns `{items: [...]}`; there is no separate `total`.
 */
final class EventTypeList
{
    /**
     * @param EventType[] $items
     */
    public function __construct(
        public readonly array $items,
    ) {}

    /**
     * @param array<string, mixed> $data
     */
    public static function fromArray(array $data): self
    {
        /** @var array<int, array<string, mixed>> $rows */
        $rows = $data['items'] ?? [];
        return new self(
            items: array_map(
                fn(array $row) => EventType::fromArray($row),
                $rows,
            ),
        );
    }
}
