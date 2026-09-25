<?php

declare(strict_types=1);

namespace FlowCatalyst\Tests\Unit\Client\Resources;

use FlowCatalyst\Client\FlowCatalystClient;
use FlowCatalyst\Client\Resources\Connections;
use FlowCatalyst\Client\Resources\Subscriptions;
use FlowCatalyst\DTOs\EventTypeBinding;
use FlowCatalyst\DTOs\Requests\SyncConnectionEntry;
use FlowCatalyst\DTOs\Requests\SyncSubscriptionEntry;
use PHPUnit\Framework\TestCase;

/**
 * Wire-level assembly for the connection/subscription sync request bodies:
 * `clientId` is a genuinely optional field (omitted, not sent as null, when
 * the sync is global) and `sharedConnection` only appears when true — both
 * additive to the pre-existing subscription sync contract.
 */
final class SyncPayloadTest extends TestCase
{
    public function test_subscriptions_sync_omits_client_id_when_null(): void
    {
        $captured = null;
        $client = $this->createMock(FlowCatalystClient::class);
        $client->expects($this->once())->method('request')
            ->willReturnCallback(function (string $method, string $endpoint, array $options) use (&$captured) {
                $captured = $options['json'];
                return ['applicationCode' => 'orders', 'created' => 0, 'updated' => 0, 'deleted' => 0, 'syncedCodes' => []];
            });

        (new Subscriptions($client))->sync('orders', [
            new SyncSubscriptionEntry(code: 'sub', name: 'Sub', target: 'https://example.com/hook', eventTypes: []),
        ]);

        $this->assertArrayNotHasKey('clientId', $captured);
    }

    public function test_subscriptions_sync_sends_client_id_when_given(): void
    {
        $captured = null;
        $client = $this->createMock(FlowCatalystClient::class);
        $client->expects($this->once())->method('request')
            ->willReturnCallback(function (string $method, string $endpoint, array $options) use (&$captured) {
                $captured = $options['json'];
                return ['applicationCode' => 'orders', 'created' => 0, 'updated' => 0, 'deleted' => 0, 'syncedCodes' => []];
            });

        (new Subscriptions($client))->sync(
            'orders',
            [new SyncSubscriptionEntry(code: 'sub', name: 'Sub', target: 'https://example.com/hook', eventTypes: [])],
            removeUnlisted: false,
            clientId: 'acme',
        );

        $this->assertSame('acme', $captured['clientId']);
    }

    public function test_connections_sync_omits_client_id_when_null_and_sends_it_when_given(): void
    {
        $captured = [];
        $client = $this->createMock(FlowCatalystClient::class);
        $client->expects($this->exactly(2))->method('request')
            ->willReturnCallback(function (string $method, string $endpoint, array $options) use (&$captured) {
                $captured[] = $options['json'];
                return ['applicationCode' => 'orders', 'created' => 0, 'updated' => 0, 'deleted' => 0, 'syncedCodes' => []];
            });

        $connections = new Connections($client);
        $connections->sync('orders', [new SyncConnectionEntry(code: 'c', name: 'C')]);
        $connections->sync('orders', [new SyncConnectionEntry(code: 'c', name: 'C')], clientId: 'acme');

        $this->assertArrayNotHasKey('clientId', $captured[0]);
        $this->assertSame('acme', $captured[1]['clientId']);
    }

    public function test_shared_connection_only_appears_in_payload_when_true(): void
    {
        $bare = new SyncSubscriptionEntry(
            code: 'sub', name: 'Sub', target: 'https://example.com/hook', eventTypes: [],
            connectionCode: 'orders-webhook',
        );
        $shared = new SyncSubscriptionEntry(
            code: 'sub', name: 'Sub', target: 'https://example.com/hook', eventTypes: [],
            connectionCode: 'orders-webhook', sharedConnection: true,
        );

        $this->assertArrayNotHasKey('sharedConnection', $bare->toArray());
        $this->assertTrue($shared->toArray()['sharedConnection']);
    }

    public function test_connection_entry_carries_only_its_own_fields(): void
    {
        $entry = new SyncConnectionEntry(code: 'orders-webhook', name: 'Orders Webhook', description: 'd', externalId: 'ext-1');

        $this->assertSame([
            'code' => 'orders-webhook',
            'name' => 'Orders Webhook',
            'description' => 'd',
            'externalId' => 'ext-1',
        ], $entry->toArray());
    }
}
