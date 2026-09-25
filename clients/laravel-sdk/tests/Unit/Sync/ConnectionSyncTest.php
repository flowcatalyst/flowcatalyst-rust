<?php

declare(strict_types=1);

namespace FlowCatalyst\Tests\Unit\Sync;

use FlowCatalyst\Client\FlowCatalystClient;
use FlowCatalyst\Client\Resources\Connections;
use FlowCatalyst\Client\Resources\Subscriptions;
use FlowCatalyst\DTOs\Responses\SyncResult as ResponseSyncResult;
use FlowCatalyst\Sync\ConnectionDefinition;
use FlowCatalyst\Sync\DefinitionSynchronizer;
use FlowCatalyst\Sync\SubscriptionDefinition;
use FlowCatalyst\Sync\SyncDefinitionSet;
use PHPUnit\Framework\TestCase;

/**
 * Connections are synced before subscriptions, both grouped by resolved
 * client — mirroring the scheduled-jobs clientId grouping — and a group
 * whose connection sync fails must not go on to sync that group's
 * subscriptions (their connection codes may not resolve).
 */
final class ConnectionSyncTest extends TestCase
{
    private function synchronizer(Connections $connections, Subscriptions $subscriptions): DefinitionSynchronizer
    {
        $client = $this->createMock(FlowCatalystClient::class);
        $client->method('connections')->willReturn($connections);
        $client->method('subscriptions')->willReturn($subscriptions);

        return new DefinitionSynchronizer($client, subscriptionTargetBaseUrl: 'https://app.example.com');
    }

    public function test_connections_are_synced_before_subscriptions(): void
    {
        $order = [];

        $connections = $this->createMock(Connections::class);
        $connections->expects($this->once())->method('sync')
            ->willReturnCallback(function () use (&$order) {
                $order[] = 'connections';
                return new ResponseSyncResult('orders', 1, 0, 0, []);
            });

        $subscriptions = $this->createMock(Subscriptions::class);
        $subscriptions->expects($this->once())->method('sync')
            ->willReturnCallback(function () use (&$order) {
                $order[] = 'subscriptions';
                return new ResponseSyncResult('orders', 1, 0, 0, []);
            });

        $set = SyncDefinitionSet::forApplication('orders')
            ->withConnections([ConnectionDefinition::make('conn-1', 'Conn 1')])
            ->withSubscriptions([
                SubscriptionDefinition::make('sub-1', 'Sub 1', '/hook', 'conn-1', 'q', 'default'),
            ]);

        $result = $this->synchronizer($connections, $subscriptions)->sync($set);

        $this->assertSame(['connections', 'subscriptions'], $order);
        $this->assertSame(1, $result->connections['created']);
        $this->assertSame(1, $result->subscriptions['created']);
        $this->assertFalse($result->hasErrors());
    }

    public function test_definitions_for_two_clients_and_no_client_produce_three_separate_calls_each(): void
    {
        $connectionCalls = [];
        $connections = $this->createMock(Connections::class);
        $connections->expects($this->exactly(3))->method('sync')
            ->willReturnCallback(function (string $appCode, array $entries, ?string $clientId, bool $removeUnlisted) use (&$connectionCalls) {
                $connectionCalls[] = $clientId;
                $this->assertCount(1, $entries);
                return new ResponseSyncResult($appCode, 1, 0, 0, []);
            });

        $subscriptionCalls = [];
        $subscriptions = $this->createMock(Subscriptions::class);
        $subscriptions->expects($this->exactly(3))->method('sync')
            ->willReturnCallback(function (string $appCode, array $entries, bool $removeUnlisted, ?string $clientId) use (&$subscriptionCalls) {
                $subscriptionCalls[] = $clientId;
                $this->assertCount(1, $entries);
                return new ResponseSyncResult($appCode, 1, 0, 0, []);
            });

        $set = SyncDefinitionSet::forApplication('orders')
            ->withConnections([
                ConnectionDefinition::make('conn-a', 'Conn A')->forClient('client-a'),
                ConnectionDefinition::make('conn-b', 'Conn B')->forClient('client-b'),
                ConnectionDefinition::make('conn-none', 'Conn None'),
            ])
            ->withSubscriptions([
                SubscriptionDefinition::make('sub-a', 'Sub A', '/hook', 'conn-a', 'q', 'default')->forClient('client-a'),
                SubscriptionDefinition::make('sub-b', 'Sub B', '/hook', 'conn-b', 'q', 'default')->forClient('client-b'),
                SubscriptionDefinition::make('sub-none', 'Sub None', '/hook', 'conn-none', 'q', 'default'),
            ]);

        $result = $this->synchronizer($connections, $subscriptions)->sync($set);

        sort($connectionCalls);
        sort($subscriptionCalls);
        $this->assertSame([null, 'client-a', 'client-b'], $connectionCalls);
        $this->assertSame([null, 'client-a', 'client-b'], $subscriptionCalls);

        // Never merged: each call synced exactly one entry (asserted above),
        // and the summed result reflects all three groups.
        $this->assertSame(3, $result->connections['created']);
        $this->assertSame(3, $result->subscriptions['created']);
    }

    public function test_connection_failure_for_a_group_skips_that_groups_subscription_sync(): void
    {
        $connections = $this->createMock(Connections::class);
        $connections->expects($this->once())->method('sync')
            ->willThrowException(new \RuntimeException('platform unavailable'));

        $subscriptions = $this->createMock(Subscriptions::class);
        $subscriptions->expects($this->never())->method('sync');

        $set = SyncDefinitionSet::forApplication('orders')
            ->withConnections([ConnectionDefinition::make('conn-1', 'Conn 1')])
            ->withSubscriptions([
                SubscriptionDefinition::make('sub-1', 'Sub 1', '/hook', 'conn-1', 'q', 'default'),
            ]);

        $result = $this->synchronizer($connections, $subscriptions)->sync($set);

        $this->assertTrue($result->hasErrors());
        $this->assertStringContainsString('platform unavailable', $result->connections['error']);
        $this->assertArrayHasKey('error', $result->subscriptions);
        $this->assertStringContainsString('connection sync failed', $result->subscriptions['error']);
    }

    public function test_global_group_is_synced_before_client_groups_within_one_set(): void
    {
        $order = [];

        $connections = $this->createMock(Connections::class);
        $connections->method('sync')
            ->willReturnCallback(function (string $appCode, array $entries, ?string $clientId) use (&$order) {
                $order[] = 'conn:' . ($clientId ?? 'global');
                return new ResponseSyncResult($appCode, 1, 0, 0, []);
            });

        $subscriptions = $this->createMock(Subscriptions::class);
        $subscriptions->method('sync')
            ->willReturnCallback(function (string $appCode, array $entries, bool $removeUnlisted, ?string $clientId) use (&$order) {
                $order[] = 'sub:' . ($clientId ?? 'global');
                return new ResponseSyncResult($appCode, 1, 0, 0, []);
            });

        $set = SyncDefinitionSet::forApplication('orders')
            ->withConnections([
                ConnectionDefinition::make('conn-a', 'Conn A')->forClient('client-a'),
                ConnectionDefinition::make('conn-none', 'Conn None'),
            ])
            ->withSubscriptions([
                SubscriptionDefinition::make('sub-a', 'Sub A', '/hook', 'conn-a', 'q', 'default')->forClient('client-a'),
                SubscriptionDefinition::make('sub-none', 'Sub None', '/hook', 'conn-none', 'q', 'default'),
            ]);

        $this->synchronizer($connections, $subscriptions)->sync($set);

        $this->assertSame('conn:global', $order[0]);
        $this->assertSame('sub:global', $order[1]);
        // The client-a group's connections+subscriptions come after the global pair.
        $this->assertContains('conn:client-a', array_slice($order, 2));
        $this->assertContains('sub:client-a', array_slice($order, 2));
    }
}
