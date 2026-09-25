<?php

declare(strict_types=1);

namespace FlowCatalyst\Tests\Feature\Sync;

use FlowCatalyst\Client\FlowCatalystClient;
use FlowCatalyst\Client\Resources\Connections;
use FlowCatalyst\Definition\DefinitionRepository;
use FlowCatalyst\DTOs\Responses\SyncResult as ResponseSyncResult;
use FlowCatalyst\FlowCatalystServiceProvider;
use FlowCatalyst\Tests\Fixtures\Sync\FakeSetProvider;
use Illuminate\Support\Facades\Artisan;
use Orchestra\Testbench\TestCase;

/**
 * `flowcatalyst:sync` resolves every class configured in
 * `flowcatalyst.definitions.set_providers` from the container and syncs
 * every `SyncDefinitionSet` it yields, in addition to the scanned attribute
 * definitions — the multi-tenant path (see ProvidesSyncDefinitionSets).
 */
final class SetProviderCommandTest extends TestCase
{
    protected function getPackageProviders($app): array
    {
        return [FlowCatalystServiceProvider::class];
    }

    protected function getEnvironmentSetUp($app): void
    {
        $app['config']->set('flowcatalyst.base_url', 'https://fc.test');
        $app['config']->set('flowcatalyst.definitions.set_providers', [FakeSetProvider::class]);
        $app['config']->set('flowcatalyst.definitions.seed_spatie', false);
    }

    public function test_provider_sets_are_resolved_and_synced(): void
    {
        // No attribute definitions scanned — an empty cache is enough for
        // the command to proceed past its cache-exists guard.
        $this->app->make(DefinitionRepository::class)->scanAndCache([]);

        $syncedAppCodes = [];
        $connections = $this->createMock(Connections::class);
        $connections->expects($this->once())->method('sync')
            ->willReturnCallback(function (string $appCode, array $entries) use (&$syncedAppCodes) {
                $syncedAppCodes[] = $appCode;
                return new ResponseSyncResult($appCode, 1, 0, 0, ['conn-1']);
            });

        $client = $this->createMock(FlowCatalystClient::class);
        $client->method('connections')->willReturn($connections);
        $this->app->instance(FlowCatalystClient::class, $client);

        $exitCode = Artisan::call('flowcatalyst:sync');

        $this->assertSame(0, $exitCode);
        $this->assertSame(['orders'], $syncedAppCodes);
    }

    /**
     * When the provider yields a set for the SAME application the scanned
     * attribute definitions belong to, the two must be merged into ONE
     * platform call — never two separate calls for the same (application,
     * client) scope (see MergedSyncTest for the underlying bug/fix).
     */
    public function test_provider_set_for_the_same_application_as_scanned_attributes_merges_into_one_call(): void
    {
        $this->app['config']->set('flowcatalyst.application_code', 'orders');
        $this->app->make(DefinitionRepository::class)->scanAndCache([
            __DIR__ . '/../../Fixtures/Connections',
        ]);

        // Calls grouped by application code — the scanner's fixtures also
        // include a connection for a DIFFERENT application ('billing'),
        // which must stay in its own, separate call.
        $callsByApp = [];
        $connections = $this->createMock(Connections::class);
        $connections->method('sync')
            ->willReturnCallback(function (string $appCode, array $entries) use (&$callsByApp) {
                $callsByApp[$appCode][] = array_map(fn($e) => $e->code, $entries);
                return new ResponseSyncResult($appCode, count($entries), 0, 0, []);
            });

        $client = $this->createMock(FlowCatalystClient::class);
        $client->method('connections')->willReturn($connections);
        $this->app->instance(FlowCatalystClient::class, $client);

        $exitCode = Artisan::call('flowcatalyst:sync');

        $this->assertSame(0, $exitCode);
        $this->assertArrayHasKey('orders', $callsByApp);
        $this->assertCount(1, $callsByApp['orders'], 'the scanned connection and the provider connection must arrive in ONE call for "orders"');
        sort($callsByApp['orders'][0]);
        // 'orders-webhook' is the scanned, default-app-scoped connection;
        // 'conn-1' is FakeSetProvider's.
        $this->assertSame(['conn-1', 'orders-webhook'], $callsByApp['orders'][0]);
    }

    public function test_unresolvable_provider_is_reported_and_does_not_abort_the_sync(): void
    {
        $this->app->make(DefinitionRepository::class)->scanAndCache([]);
        $this->app['config']->set('flowcatalyst.definitions.set_providers', [
            'App\\NonExistent\\NotARealClass',
            FakeSetProvider::class,
        ]);

        $connections = $this->createMock(Connections::class);
        $connections->expects($this->once())->method('sync')
            ->willReturn(new ResponseSyncResult('orders', 1, 0, 0, ['conn-1']));

        $client = $this->createMock(FlowCatalystClient::class);
        $client->method('connections')->willReturn($connections);
        $this->app->instance(FlowCatalystClient::class, $client);

        $exitCode = Artisan::call('flowcatalyst:sync');

        $this->assertSame(0, $exitCode);
        $this->assertStringContainsString('Could not resolve FlowCatalyst set provider', Artisan::output());
    }
}
