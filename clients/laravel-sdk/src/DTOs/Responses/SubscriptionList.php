<?php

declare(strict_types=1);

namespace FlowCatalyst\DTOs\Responses;

use FlowCatalyst\DTOs\Subscription;

/**
 * Paginated list of subscriptions returned by GET /api/subscriptions.
 */
final class SubscriptionList
{
    /**
     * @param Subscription[] $subscriptions
     */
    public function __construct(
        public readonly array $subscriptions,
        public readonly int $total,
    ) {}

    /**
     * @param array<string, mixed> $data
     */
    public static function fromArray(array $data): self
    {
        /** @var array<int, array<string, mixed>> $rows */
        $rows = $data['subscriptions'] ?? [];
        return new self(
            subscriptions: array_map(
                fn(array $row) => Subscription::fromArray($row),
                $rows,
            ),
            total: (int) ($data['total'] ?? count($rows)),
        );
    }
}
