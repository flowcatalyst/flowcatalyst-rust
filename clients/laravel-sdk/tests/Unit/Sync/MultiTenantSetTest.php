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
 * A multi-tenant application builds one `SyncDefinitionSet` per
 * (application, client) programmatically. `forClient(..., targetBaseUrl:)`
 * scopes a set to a client and, for THAT set only, overrides the base URL a
 * path-style subscription target resolves against (tenants often have their
 * own host). Across sets sharing an application code, the global set must
 * be synced — in full, including its connections — before any client set.
 */
final class MultiTenantSetTest extends TestCase
{
    private function synchronizer(Subscriptions $subscriptions, ?string $defaultBaseUrl = 'https://default.example.com'): DefinitionSynchronizer
    {
        $connections = $this->createMock(Connections::class);
        $connections->method('sync')->willReturn(new ResponseSyncResult('orders', 0, 0, 0, []));

        $client = $this->createMock(FlowCatalystClient::class);
        $client->method('connections')->willReturn($connections);
        $client->method('subscriptions')->willReturn($subscriptions);

        return new DefinitionSynchronizer($client, subscriptionTargetBaseUrl: $defaultBaseUrl);
    }

    public function test_forclient_target_base_url_overrides_the_default_for_that_set_only(): void
    {
        $sent = null;
        $subscriptions = $this->createMock(Subscriptions::class);
        $subscriptions->expects($this->once())->method('sync')
            ->willReturnCallback(function (string $appCode, array $entries) use (&$sent) {
                $sent = $entries[0]->target;
                return new ResponseSyncResult($appCode, 1, 0, 0, []);
            });

        $set = SyncDefinitionSet::forApplication('orders')
            ->forClient('acme', targetBaseUrl: 'https://acme.example.com')
            ->withSubscriptions([
                SubscriptionDefinition::make('sub-1', 'Sub 1', '/webhooks/orders', 'conn-1', 'q', 'default'),
            ]);

        $this->synchronizer($subscriptions)->sync($set);

        $this->assertSame('https://acme.example.com/webhooks/orders', $sent);
    }

    public function test_forclient_without_target_base_url_falls_back_to_the_synchronizers_default(): void
    {
        $sent = null;
        $subscriptions = $this->createMock(Subscriptions::class);
        $subscriptions->expects($this->once())->method('sync')
            ->willReturnCallback(function (string $appCode, array $entries) use (&$sent) {
                $sent = $entries[0]->target;
                return new ResponseSyncResult($appCode, 1, 0, 0, []);
            });

        $set = SyncDefinitionSet::forApplication('orders')
            ->forClient('acme')
            ->withSubscriptions([
                SubscriptionDefinition::make('sub-1', 'Sub 1', '/webhooks/orders', 'conn-1', 'q', 'default'),
            ]);

        $this->synchronizer($subscriptions, 'https://default.example.com')->sync($set);

        $this->assertSame('https://default.example.com/webhooks/orders', $sent);
    }

    public function test_absolute_target_is_untouched_by_the_sets_target_base_url(): void
    {
        $sent = null;
        $subscriptions = $this->createMock(Subscriptions::class);
        $subscriptions->expects($this->once())->method('sync')
            ->willReturnCallback(function (string $appCode, array $entries) use (&$sent) {
                $sent = $entries[0]->target;
                return new ResponseSyncResult($appCode, 1, 0, 0, []);
            });

        $set = SyncDefinitionSet::forApplication('orders')
            ->forClient('acme', targetBaseUrl: 'https://acme.example.com')
            ->withSubscriptions([
                SubscriptionDefinition::make('sub-1', 'Sub 1', 'https://elsewhere.example.org/hook', 'conn-1', 'q', 'default'),
            ]);

        $this->synchronizer($subscriptions)->sync($set);

        $this->assertSame('https://elsewhere.example.org/hook', $sent);
    }

    public function test_global_set_is_synced_before_client_sets_for_the_same_application(): void
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

        $client = $this->createMock(FlowCatalystClient::class);
        $client->method('connections')->willReturn($connections);
        $client->method('subscriptions')->willReturn($subscriptions);
        $synchronizer = new DefinitionSynchronizer($client, subscriptionTargetBaseUrl: 'https://default.example.com');

        // Deliberately built client-set first, global-set second — ordering
        // must not depend on the order sets are passed in.
        $clientSet = SyncDefinitionSet::forApplication('orders')
            ->forClient('acme')
            ->withConnections([ConnectionDefinition::make('conn-acme', 'Conn Acme')])
            ->withSubscriptions([SubscriptionDefinition::make('sub-acme', 'Sub Acme', '/hook', 'conn-acme', 'q', 'default')]);

        $globalSet = SyncDefinitionSet::forApplication('orders')
            ->withConnections([ConnectionDefinition::make('conn-global', 'Conn Global')])
            ->withSubscriptions([SubscriptionDefinition::make('sub-global', 'Sub Global', '/hook', 'conn-global', 'q', 'default')]);

        $results = $synchronizer->syncGrouped([$clientSet, $globalSet]);

        $this->assertSame(['conn:global', 'sub:global', 'conn:acme', 'sub:acme'], $order);

        // Results for both sets (same application code) are summed into one.
        $this->assertArrayHasKey('orders', $results);
        $this->assertSame(2, $results['orders']->connections['created']);
        $this->assertSame(2, $results['orders']->subscriptions['created']);
    }

    public function test_syncgrouped_keeps_different_applications_separate(): void
    {
        $connections = $this->createMock(Connections::class);
        $connections->method('sync')->willReturn(new ResponseSyncResult('x', 1, 0, 0, []));
        $subscriptions = $this->createMock(Subscriptions::class);
        $subscriptions->method('sync')->willReturn(new ResponseSyncResult('x', 1, 0, 0, []));

        $client = $this->createMock(FlowCatalystClient::class);
        $client->method('connections')->willReturn($connections);
        $client->method('subscriptions')->willReturn($subscriptions);
        $synchronizer = new DefinitionSynchronizer($client, subscriptionTargetBaseUrl: 'https://default.example.com');

        $setA = SyncDefinitionSet::forApplication('app-a')
            ->withConnections([ConnectionDefinition::make('c', 'C')]);
        $setB = SyncDefinitionSet::forApplication('app-b')
            ->withConnections([ConnectionDefinition::make('c', 'C')]);

        $results = $synchronizer->syncGrouped([$setA, $setB]);

        $this->assertSame(['app-a', 'app-b'], array_keys($results));
        $this->assertSame(1, $results['app-a']->connections['created']);
        $this->assertSame(1, $results['app-b']->connections['created']);
    }
}
