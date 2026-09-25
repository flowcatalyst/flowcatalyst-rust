<?php

declare(strict_types=1);

namespace FlowCatalyst\Tests\Unit\Sync;

use FlowCatalyst\Attributes\AsSubscription;
use FlowCatalyst\Client\FlowCatalystClient;
use FlowCatalyst\Client\Resources\Subscriptions;
use FlowCatalyst\DTOs\Requests\SyncSubscriptionEntry;
use FlowCatalyst\DTOs\Responses\SyncResult;
use FlowCatalyst\Sync\DefinitionSynchronizer;
use FlowCatalyst\Sync\SubscriptionDefinition;
use FlowCatalyst\Sync\SyncDefinitionSet;
use PHPUnit\Framework\TestCase;

/**
 * The platform's subscription sync requires `target`, but #[AsSubscription]
 * had no such field — so every code-first subscription synced with an empty
 * target and was rejected. The attribute now carries `target`, and a path is
 * resolved against a base URL at sync time so one definition serves every
 * environment.
 */
final class SubscriptionTargetResolutionTest extends TestCase
{
    public function test_attribute_emits_target(): void
    {
        $row = $this->attribute(target: '/webhooks/orders')->toArray();

        $this->assertSame('/webhooks/orders', $row['target']);
    }

    public function test_path_target_is_resolved_against_the_base_url(): void
    {
        $sent = $this->syncedTargets(
            [$this->attribute(target: '/webhooks/orders')->toArray()],
            baseUrl: 'https://tenant.example.com/',
        );

        $this->assertSame(['https://tenant.example.com/webhooks/orders'], $sent);
    }

    public function test_path_without_leading_slash_still_joins_cleanly(): void
    {
        $sent = $this->syncedTargets(
            [$this->attribute(target: 'webhooks/orders')->toArray()],
            baseUrl: 'https://tenant.example.com',
        );

        $this->assertSame(['https://tenant.example.com/webhooks/orders'], $sent);
    }

    public function test_absolute_target_is_sent_verbatim(): void
    {
        $sent = $this->syncedTargets(
            [$this->attribute(target: 'https://hooks.example.org/orders')->toArray()],
            baseUrl: 'https://tenant.example.com',
        );

        $this->assertSame(['https://hooks.example.org/orders'], $sent);
    }

    /**
     * `target` is a required constructor argument, so an attribute can't omit
     * it — but it can be blank, and rows handed to withSubscriptions() are
     * plain arrays that may lack the key altogether.
     */
    public function test_blank_or_absent_target_fails_the_sync_without_calling_the_platform(): void
    {
        $withoutKey = $this->attribute(code: 'no-key')->toArray();
        unset($withoutKey['target']);

        $result = $this->syncExpectingNoCall(
            [
                $this->attribute()->toArray(),
                $this->attribute(code: 'blank', target: '  ')->toArray(),
                $withoutKey,
            ],
            baseUrl: 'https://tenant.example.com',
        );

        $this->assertStringContainsString('blank', $result['error']);
        $this->assertStringContainsString('no-key', $result['error']);
        $this->assertStringNotContainsString('order-processor', $result['error']);
    }

    public function test_path_target_with_no_base_url_fails_the_sync(): void
    {
        $result = $this->syncExpectingNoCall(
            [$this->attribute(target: '/webhooks/orders')->toArray()],
            baseUrl: null,
        );

        $this->assertStringContainsString('order-processor', $result['error']);
    }

    /**
     * A connection id differs per environment; its code doesn't. The payload
     * must carry the code and no id, leaving the platform to resolve it.
     */
    public function test_connection_is_sent_by_code_not_id(): void
    {
        $payload = null;
        $subscriptions = $this->createMock(Subscriptions::class);
        $subscriptions->expects($this->once())->method('sync')
            ->willReturnCallback(function (string $appCode, array $entries) use (&$payload) {
                $payload = $entries[0]->toArray();
                return new SyncResult($appCode, 1, 0, 0, []);
            });

        $this->synchronizer($subscriptions, 'https://tenant.example.com')
            ->sync(SyncDefinitionSet::forApplication('orders')->withSubscriptions([$this->attribute()->toArray()]));

        $this->assertSame('orders-webhook', $payload['connectionCode']);
        $this->assertArrayNotHasKey('connectionId', $payload);
    }

    public function test_programmatic_definition_keeps_its_target_across_fluent_copies(): void
    {
        $definition = SubscriptionDefinition::make('billing', 'Billing', '/webhooks/billing', 'conn_1', 'integrations', 'default')
            ->withDescription('Billing integration')
            ->clientScoped()
            ->forEventType('myapp:billing:invoice:created')
            ->withRetry(5)
            ->withTimeout(10)
            ->inactive();

        $this->assertSame('/webhooks/billing', $definition->toArray()['target']);
        $this->assertSame(
            '/webhooks/billing',
            SubscriptionDefinition::fromArray($definition->toArray())->target,
        );
    }

    public function test_programmatic_definition_target_is_resolved_on_sync(): void
    {
        $definition = SubscriptionDefinition::make('billing', 'Billing', '/old', 'conn_1', 'integrations', 'default')
            ->withTarget('/webhooks/billing')
            ->forEventType('myapp:billing:invoice:created');

        $sent = $this->syncedTargets([$definition->toArray()], baseUrl: 'https://crm.example.com');

        $this->assertSame(['https://crm.example.com/webhooks/billing'], $sent);
    }

    private function attribute(string $code = 'order-processor', string $target = '/webhooks/orders'): AsSubscription
    {
        return new AsSubscription(
            code: $code,
            name: 'Order Processor',
            target: $target,
            connectionCode: 'orders-webhook',
            queue: 'orders',
            dispatchPoolCode: 'default',
            eventTypes: ['myapp:orders:order:created'],
        );
    }

    /**
     * @param array<array<string, mixed>> $rows
     * @return string[] the target of each entry handed to the platform client
     */
    private function syncedTargets(array $rows, ?string $baseUrl): array
    {
        $sent = [];
        $subscriptions = $this->createMock(Subscriptions::class);
        $subscriptions->expects($this->once())
            ->method('sync')
            ->willReturnCallback(function (string $appCode, array $entries) use (&$sent) {
                $sent = array_map(fn(SyncSubscriptionEntry $e) => $e->target, $entries);
                return new SyncResult($appCode, count($entries), 0, 0, []);
            });

        $result = $this->synchronizer($subscriptions, $baseUrl)
            ->sync(SyncDefinitionSet::forApplication('orders')->withSubscriptions($rows));

        $this->assertArrayNotHasKey('error', $result->subscriptions);

        return $sent;
    }

    /**
     * @param array<array<string, mixed>> $rows
     * @return array<string, mixed>
     */
    private function syncExpectingNoCall(array $rows, ?string $baseUrl): array
    {
        $subscriptions = $this->createMock(Subscriptions::class);
        // A partial sync under removeUnlisted would delete the omitted
        // subscriptions, so nothing at all may reach the platform.
        $subscriptions->expects($this->never())->method('sync');

        $result = $this->synchronizer($subscriptions, $baseUrl)
            ->sync(SyncDefinitionSet::forApplication('orders')->withSubscriptions($rows));

        $this->assertArrayHasKey('error', $result->subscriptions);

        return $result->subscriptions;
    }

    private function synchronizer(Subscriptions $subscriptions, ?string $baseUrl): DefinitionSynchronizer
    {
        $client = $this->createMock(FlowCatalystClient::class);
        $client->method('subscriptions')->willReturn($subscriptions);

        return new DefinitionSynchronizer($client, subscriptionTargetBaseUrl: $baseUrl);
    }
}
