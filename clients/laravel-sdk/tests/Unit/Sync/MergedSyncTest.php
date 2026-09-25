<?php

declare(strict_types=1);

namespace FlowCatalyst\Tests\Unit\Sync;

use FlowCatalyst\Client\FlowCatalystClient;
use FlowCatalyst\Client\Resources\Connections;
use FlowCatalyst\Client\Resources\EventTypes;
use FlowCatalyst\Client\Resources\Subscriptions;
use FlowCatalyst\DTOs\Requests\SyncSubscriptionEntry;
use FlowCatalyst\DTOs\Responses\SyncResult as ResponseSyncResult;
use FlowCatalyst\Sync\ConnectionDefinition;
use FlowCatalyst\Sync\DefinitionSynchronizer;
use FlowCatalyst\Sync\EventTypeDefinition;
use FlowCatalyst\Sync\SubscriptionDefinition;
use FlowCatalyst\Sync\SyncDefinitionSet;
use FlowCatalyst\Sync\SyncOptions;
use PHPUnit\Framework\TestCase;

/**
 * `syncGrouped()` must merge every `SyncDefinitionSet` sharing an
 * application code into ONE combined set before syncing — never sync them
 * as separate calls. The platform scopes `removeUnlisted` to one
 * (application, client) PER CALL: everything of that scope not present in
 * the call is deleted, so splitting one scope (e.g. the scanned attribute
 * definitions AND a provider's global set for the same application) across
 * two calls would let the second call delete what the first just created.
 */
final class MergedSyncTest extends TestCase
{
    /**
     * @return array{0: DefinitionSynchronizer, 1: Connections, 2: Subscriptions, 3: EventTypes}
     */
    private function synchronizer(): array
    {
        $connections = $this->createMock(Connections::class);
        $subscriptions = $this->createMock(Subscriptions::class);
        $eventTypes = $this->createMock(EventTypes::class);

        $client = $this->createMock(FlowCatalystClient::class);
        $client->method('connections')->willReturn($connections);
        $client->method('subscriptions')->willReturn($subscriptions);
        $client->method('eventTypes')->willReturn($eventTypes);

        $synchronizer = new DefinitionSynchronizer($client, subscriptionTargetBaseUrl: 'https://default.example.com');

        return [$synchronizer, $connections, $subscriptions, $eventTypes];
    }

    /**
     * THE BUG, made concrete: two global sets for one application (standing
     * in for "scanned attribute definitions" + "a provider's global set"),
     * removeUnlisted ON. Before the fix this issued TWO connections calls
     * and TWO subscriptions calls for the global scope — the second
     * deleting what the first had just created. After the fix: exactly one
     * call per resource, containing BOTH definitions.
     */
    public function test_two_global_sets_for_one_application_produce_one_call_each_containing_both(): void
    {
        [$synchronizer, $connections, $subscriptions] = $this->synchronizer();

        $connectionCalls = [];
        $connections->method('sync')
            ->willReturnCallback(function (string $appCode, array $entries, ?string $clientId, bool $removeUnlisted) use (&$connectionCalls) {
                $connectionCalls[] = ['clientId' => $clientId, 'removeUnlisted' => $removeUnlisted, 'codes' => array_map(fn($e) => $e->code, $entries)];
                return new ResponseSyncResult($appCode, count($entries), 0, 0, []);
            });

        $subscriptionCalls = [];
        $subscriptions->method('sync')
            ->willReturnCallback(function (string $appCode, array $entries, bool $removeUnlisted, ?string $clientId) use (&$subscriptionCalls) {
                $subscriptionCalls[] = ['clientId' => $clientId, 'removeUnlisted' => $removeUnlisted, 'codes' => array_map(fn($e) => $e->code, $entries)];
                return new ResponseSyncResult($appCode, count($entries), 0, 0, []);
            });

        $scannedSet = SyncDefinitionSet::forApplication('orders')
            ->withConnections([ConnectionDefinition::make('conn-scanned', 'Scanned Conn')])
            ->withSubscriptions([SubscriptionDefinition::make('sub-scanned', 'Scanned Sub', '/hook-a', 'conn-scanned', 'q', 'default')]);

        $providerSet = SyncDefinitionSet::forApplication('orders')
            ->withConnections([ConnectionDefinition::make('conn-provider', 'Provider Conn')])
            ->withSubscriptions([SubscriptionDefinition::make('sub-provider', 'Provider Sub', '/hook-b', 'conn-provider', 'q', 'default')]);

        $results = $synchronizer->syncGrouped([$scannedSet, $providerSet], SyncOptions::withRemoveUnlisted());

        $this->assertCount(1, $connectionCalls, 'expected exactly one connections call for the global scope');
        $this->assertCount(1, $subscriptionCalls, 'expected exactly one subscriptions call for the global scope');

        $this->assertNull($connectionCalls[0]['clientId']);
        $this->assertTrue($connectionCalls[0]['removeUnlisted']);
        sort($connectionCalls[0]['codes']);
        $this->assertSame(['conn-provider', 'conn-scanned'], $connectionCalls[0]['codes']);

        $this->assertNull($subscriptionCalls[0]['clientId']);
        $this->assertTrue($subscriptionCalls[0]['removeUnlisted']);
        sort($subscriptionCalls[0]['codes']);
        $this->assertSame(['sub-provider', 'sub-scanned'], $subscriptionCalls[0]['codes']);

        $this->assertFalse($results['orders']->hasErrors());
        $this->assertSame(2, $results['orders']->connections['created']);
        $this->assertSame(2, $results['orders']->subscriptions['created']);
    }

    /**
     * Same bug, same fix, for two sets bound to the SAME client.
     */
    public function test_two_sets_for_the_same_client_produce_one_call_each_containing_both(): void
    {
        [$synchronizer, $connections, $subscriptions] = $this->synchronizer();

        $connectionCalls = [];
        $connections->method('sync')
            ->willReturnCallback(function (string $appCode, array $entries, ?string $clientId) use (&$connectionCalls) {
                $connectionCalls[] = ['clientId' => $clientId, 'codes' => array_map(fn($e) => $e->code, $entries)];
                return new ResponseSyncResult($appCode, count($entries), 0, 0, []);
            });

        $subscriptionCalls = [];
        $subscriptions->method('sync')
            ->willReturnCallback(function (string $appCode, array $entries, bool $removeUnlisted, ?string $clientId) use (&$subscriptionCalls) {
                $subscriptionCalls[] = ['clientId' => $clientId, 'codes' => array_map(fn($e) => $e->code, $entries)];
                return new ResponseSyncResult($appCode, count($entries), 0, 0, []);
            });

        $setOne = SyncDefinitionSet::forApplication('orders')->forClient('acme')
            ->withConnections([ConnectionDefinition::make('conn-one', 'Conn One')])
            ->withSubscriptions([SubscriptionDefinition::make('sub-one', 'Sub One', '/hook-a', 'conn-one', 'q', 'default')]);

        $setTwo = SyncDefinitionSet::forApplication('orders')->forClient('acme')
            ->withConnections([ConnectionDefinition::make('conn-two', 'Conn Two')])
            ->withSubscriptions([SubscriptionDefinition::make('sub-two', 'Sub Two', '/hook-b', 'conn-two', 'q', 'default')]);

        $synchronizer->syncGrouped([$setOne, $setTwo], SyncOptions::withRemoveUnlisted());

        $this->assertCount(1, $connectionCalls);
        $this->assertCount(1, $subscriptionCalls);
        $this->assertSame('acme', $connectionCalls[0]['clientId']);
        $this->assertSame('acme', $subscriptionCalls[0]['clientId']);
        sort($connectionCalls[0]['codes']);
        sort($subscriptionCalls[0]['codes']);
        $this->assertSame(['conn-one', 'conn-two'], $connectionCalls[0]['codes']);
        $this->assertSame(['sub-one', 'sub-two'], $subscriptionCalls[0]['codes']);
    }

    /**
     * Non-client-scoped categories (event types here) from two sets of one
     * application must also arrive in ONE call — they're scoped per
     * application only, so the same one-call-per-scope rule applies.
     */
    public function test_event_types_from_two_sets_arrive_in_one_call(): void
    {
        [$synchronizer, , , $eventTypes] = $this->synchronizer();

        $calls = [];
        $eventTypes->method('sync')
            ->willReturnCallback(function (string $appCode, array $entries, bool $removeUnlisted) use (&$calls) {
                $calls[] = array_map(fn($e) => $e->code, $entries);
                return new ResponseSyncResult($appCode, count($entries), 0, 0, []);
            });

        $setOne = SyncDefinitionSet::forApplication('orders')
            ->withEventTypes([EventTypeDefinition::make('Order Created', 'orders', 'order', 'order', 'created')]);
        $setTwo = SyncDefinitionSet::forApplication('orders')
            ->withEventTypes([EventTypeDefinition::make('Order Shipped', 'orders', 'order', 'order', 'shipped')]);

        $results = $synchronizer->syncGrouped([$setOne, $setTwo]);

        $this->assertCount(1, $calls, 'expected exactly one event-types call for the application');
        sort($calls[0]);
        $this->assertSame(['orders:order:order:created', 'orders:order:order:shipped'], $calls[0]);
        $this->assertSame(2, $results['orders']->eventTypes['created']);
    }

    /**
     * Mixed: global set + client A set + a SECOND client A set + client B
     * set. Calls must be: global, then A (the two A sets merged into one
     * call), then B — connections before subscriptions within each.
     */
    public function test_mixed_sets_merge_per_client_and_order_global_then_a_then_b(): void
    {
        [$synchronizer, $connections, $subscriptions] = $this->synchronizer();

        $order = [];
        $connections->method('sync')
            ->willReturnCallback(function (string $appCode, array $entries, ?string $clientId) use (&$order) {
                $order[] = 'conn:' . ($clientId ?? 'global') . ':' . count($entries);
                return new ResponseSyncResult($appCode, count($entries), 0, 0, []);
            });
        $subscriptions->method('sync')
            ->willReturnCallback(function (string $appCode, array $entries, bool $removeUnlisted, ?string $clientId) use (&$order) {
                $order[] = 'sub:' . ($clientId ?? 'global') . ':' . count($entries);
                return new ResponseSyncResult($appCode, count($entries), 0, 0, []);
            });

        $global = SyncDefinitionSet::forApplication('orders')
            ->withConnections([ConnectionDefinition::make('conn-global', 'Conn Global')])
            ->withSubscriptions([SubscriptionDefinition::make('sub-global', 'Sub Global', '/hook', 'conn-global', 'q', 'default')]);

        $clientAOne = SyncDefinitionSet::forApplication('orders')->forClient('client-a')
            ->withConnections([ConnectionDefinition::make('conn-a1', 'Conn A1')])
            ->withSubscriptions([SubscriptionDefinition::make('sub-a1', 'Sub A1', '/hook', 'conn-a1', 'q', 'default')]);

        $clientATwo = SyncDefinitionSet::forApplication('orders')->forClient('client-a')
            ->withConnections([ConnectionDefinition::make('conn-a2', 'Conn A2')])
            ->withSubscriptions([SubscriptionDefinition::make('sub-a2', 'Sub A2', '/hook', 'conn-a2', 'q', 'default')]);

        $clientB = SyncDefinitionSet::forApplication('orders')->forClient('client-b')
            ->withConnections([ConnectionDefinition::make('conn-b', 'Conn B')])
            ->withSubscriptions([SubscriptionDefinition::make('sub-b', 'Sub B', '/hook', 'conn-b', 'q', 'default')]);

        $synchronizer->syncGrouped([$global, $clientAOne, $clientATwo, $clientB]);

        $this->assertSame([
            'conn:global:1',
            'sub:global:1',
            'conn:client-a:2',
            'sub:client-a:2',
            'conn:client-b:1',
            'sub:client-b:1',
        ], $order);
    }

    /**
     * A set's `forClient(..., targetBaseUrl:)` must still win, per row,
     * after merging with another set that has a DIFFERENT (or no) base URL
     * — and the internal bookkeeping key used to carry it through the merge
     * must never reach the platform payload.
     */
    public function test_per_set_target_base_url_survives_the_merge_and_never_leaks_into_the_payload(): void
    {
        [$synchronizer, , $subscriptions] = $this->synchronizer();

        $sent = [];
        $subscriptions->method('sync')
            ->willReturnCallback(function (string $appCode, array $entries) use (&$sent) {
                foreach ($entries as $entry) {
                    $sent[$entry->code] = $entry;
                }
                return new ResponseSyncResult($appCode, count($entries), 0, 0, []);
            });

        $acmeSet = SyncDefinitionSet::forApplication('orders')
            ->forClient('acme', targetBaseUrl: 'https://acme.example.com')
            ->withSubscriptions([SubscriptionDefinition::make('sub-acme-one', 'One', '/hook', 'conn', 'q', 'default')]);

        // A second set for the SAME client, with no targetBaseUrl of its
        // own — its row must fall back to the SYNCHRONIZER's default
        // ('https://default.example.com'), never to acme's base URL, even
        // though it is merged into the same call.
        $acmeSetTwo = SyncDefinitionSet::forApplication('orders')
            ->forClient('acme')
            ->withSubscriptions([SubscriptionDefinition::make('sub-acme-two', 'Two', '/hook', 'conn', 'q', 'default')]);

        $synchronizer->syncGrouped([$acmeSet, $acmeSetTwo]);

        $this->assertCount(2, $sent);
        $this->assertSame('https://acme.example.com/hook', $sent['sub-acme-one']->target);
        $this->assertSame('https://default.example.com/hook', $sent['sub-acme-two']->target);

        // The internal key is not a real field of the entry DTO at all, so
        // it structurally cannot appear in toArray() — assert that directly.
        foreach ($sent as $entry) {
            $this->assertInstanceOf(SyncSubscriptionEntry::class, $entry);
            $this->assertArrayNotHasKey('_targetBaseUrl', $entry->toArray());
        }
    }

    /**
     * Duplicate connection code landing in the same (application, client)
     * scope from two different sets is a configuration error: that type's
     * sync for that scope fails locally, naming the code and the scope, and
     * the platform is never called for it.
     */
    public function test_duplicate_connection_code_across_two_sets_fails_locally_without_calling_the_platform(): void
    {
        [$synchronizer, $connections] = $this->synchronizer();
        $connections->expects($this->never())->method('sync');

        $setOne = SyncDefinitionSet::forApplication('orders')
            ->withConnections([ConnectionDefinition::make('dup-conn', 'First')]);
        $setTwo = SyncDefinitionSet::forApplication('orders')
            ->withConnections([ConnectionDefinition::make('dup-conn', 'Second')]);

        $results = $synchronizer->syncGrouped([$setOne, $setTwo]);

        $this->assertTrue($results['orders']->hasErrors());
        $error = $results['orders']->connections['error'] ?? '';
        $this->assertStringContainsString('dup-conn', $error);
        $this->assertStringContainsString('orders', $error);
    }

    /**
     * Same, for subscriptions — and the connection sync (no duplicates)
     * must still have gone through, proving the failure is scoped to the
     * colliding type only.
     */
    public function test_duplicate_subscription_code_across_two_sets_fails_locally_without_calling_the_platform(): void
    {
        [$synchronizer, $connections, $subscriptions] = $this->synchronizer();
        $connections->method('sync')->willReturn(new ResponseSyncResult('orders', 1, 0, 0, []));
        $subscriptions->expects($this->never())->method('sync');

        $setOne = SyncDefinitionSet::forApplication('orders')
            ->withConnections([ConnectionDefinition::make('conn-1', 'Conn 1')])
            ->withSubscriptions([SubscriptionDefinition::make('dup-sub', 'First', '/hook', 'conn-1', 'q', 'default')]);
        $setTwo = SyncDefinitionSet::forApplication('orders')
            ->withSubscriptions([SubscriptionDefinition::make('dup-sub', 'Second', '/hook', 'conn-1', 'q', 'default')]);

        $results = $synchronizer->syncGrouped([$setOne, $setTwo]);

        $this->assertTrue($results['orders']->hasErrors());
        $error = $results['orders']->subscriptions['error'] ?? '';
        $this->assertStringContainsString('dup-sub', $error);
        $this->assertStringContainsString('orders', $error);
        // The connection sync (no duplicates there) still went through.
        $this->assertSame(1, $results['orders']->connections['created']);
    }
}
