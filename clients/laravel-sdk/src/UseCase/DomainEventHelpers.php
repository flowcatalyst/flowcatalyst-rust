<?php

declare(strict_types=1);

namespace FlowCatalyst\UseCase;

use FlowCatalyst\Support\TsidGenerator;

/**
 * Stateless helpers for building / decoding domain event fields.
 * Kept as a static-only class (vs functions) so Laravel's PSR-4 loader
 * can find it without any shim.
 */
final class DomainEventHelpers
{
    private function __construct() {}

    public static function generateId(): string
    {
        return TsidGenerator::generate();
    }

    /** "orders.order.123" */
    public static function subject(string $domain, string $aggregate, string $id): string
    {
        return "{$domain}.{$aggregate}.{$id}";
    }

    /** "orders:order:123" */
    public static function messageGroup(string $domain, string $aggregate, string $id): string
    {
        return "{$domain}:{$aggregate}:{$id}";
    }

    /** "shop:orders:order:shipped" */
    public static function eventType(string $app, string $domain, string $aggregate, string $action): string
    {
        return "{$app}:{$domain}:{$aggregate}:{$action}";
    }

    /** "orders.order.123" → "Order" */
    public static function extractAggregateType(string $subject): string
    {
        if ($subject === '') {
            return 'Unknown';
        }
        $parts = explode('.', $subject);
        if (count($parts) < 2) {
            return 'Unknown';
        }
        $aggregate = str_replace('-', '', $parts[1]);
        return ucfirst($aggregate);
    }

    /** "orders.order.123" → "123" */
    public static function extractEntityId(string $subject): ?string
    {
        if ($subject === '') {
            return null;
        }
        $parts = explode('.', $subject);
        return count($parts) >= 3 ? $parts[2] : null;
    }
}
