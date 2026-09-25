<?php

declare(strict_types=1);

namespace FlowCatalyst\Tests\Unit\Attributes;

use FlowCatalyst\Attributes\AsConnection;
use PHPUnit\Framework\TestCase;

/**
 * #[AsConnection] carries nothing environment-specific — the platform
 * assigns the connection's own service account. `application` and `client`
 * are routing axes, resolved by the scanner (see ConnectionScanningTest),
 * and must never leak into the synced payload.
 */
final class ConnectionAttributeTest extends TestCase
{
    public function test_to_array_contains_only_code_name(): void
    {
        $attribute = new AsConnection(code: 'orders-webhook', name: 'Orders Webhook');

        $this->assertSame([
            'code' => 'orders-webhook',
            'name' => 'Orders Webhook',
        ], $attribute->toArray());
    }

    public function test_to_array_includes_optional_fields_when_set(): void
    {
        $attribute = new AsConnection(
            code: 'orders-webhook',
            name: 'Orders Webhook',
            description: 'Delivers order events',
            externalId: 'ext-123',
        );

        $this->assertSame([
            'code' => 'orders-webhook',
            'name' => 'Orders Webhook',
            'description' => 'Delivers order events',
            'externalId' => 'ext-123',
        ], $attribute->toArray());
    }

    public function test_application_and_client_never_leak_into_the_payload(): void
    {
        $attribute = new AsConnection(
            code: 'orders-webhook',
            name: 'Orders Webhook',
            application: 'billing',
            client: 'acme',
        );

        $payload = $attribute->toArray();

        $this->assertArrayNotHasKey('application', $payload);
        $this->assertArrayNotHasKey('client', $payload);
    }
}
