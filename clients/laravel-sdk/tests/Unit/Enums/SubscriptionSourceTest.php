<?php

declare(strict_types=1);

namespace FlowCatalyst\Tests\Unit\Enums;

use FlowCatalyst\DTOs\Subscription;
use FlowCatalyst\Enums\SubscriptionSource;
use PHPUnit\Framework\TestCase;

final class SubscriptionSourceTest extends TestCase
{
    public function test_from_parses_function(): void
    {
        $this->assertSame(SubscriptionSource::FUNCTION, SubscriptionSource::from('FUNCTION'));
        $this->assertSame('FUNCTION', SubscriptionSource::FUNCTION->value);
    }

    public function test_subscription_from_array_accepts_function_source(): void
    {
        $subscription = Subscription::fromArray([
            'id' => 'sub_1',
            'code' => 'orders-shipped',
            'name' => 'Orders Shipped',
            'endpoint' => 'https://example.com/webhook',
            'status' => 'ACTIVE',
            'mode' => 'IMMEDIATE',
            'eventTypes' => [],
            'customConfig' => [],
            'dataOnly' => true,
            'clientScoped' => false,
            'maxAgeSeconds' => 86400,
            'delaySeconds' => 0,
            'sequence' => 99,
            'timeoutSeconds' => 30,
            'maxRetries' => 0,
            'createdAt' => '2026-01-01T00:00:00Z',
            'updatedAt' => '2026-01-01T00:00:00Z',
            'source' => 'FUNCTION',
        ]);

        $this->assertSame('FUNCTION', $subscription->source);
        $this->assertSame(SubscriptionSource::FUNCTION, SubscriptionSource::from($subscription->source));
    }
}
