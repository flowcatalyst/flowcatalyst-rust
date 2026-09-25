<?php

declare(strict_types=1);

namespace FlowCatalyst\Tests\Unit\Definition;

use FlowCatalyst\Definition\DefinitionScanner;
use FlowCatalyst\Definition\ScannedDefinitions;
use Illuminate\Config\Repository;
use Illuminate\Container\Container;
use Illuminate\Filesystem\Filesystem;
use Illuminate\Support\Facades\Facade;
use PHPUnit\Framework\TestCase;

/**
 * #[AsConnection] is scanned like every other definition attribute: the
 * scanner resolves its application (attribute override / namespace map /
 * default) and its client (attribute override / `flowcatalyst.client`
 * default), and the scanned rows round-trip through the JSON cache
 * (ScannedDefinitions::toArray()/fromArray()) used by `flowcatalyst:scan` /
 * `flowcatalyst:sync`.
 */
final class ConnectionScanningTest extends TestCase
{
    protected function setUp(): void
    {
        $container = new Container();
        Container::setInstance($container);
        Facade::setFacadeApplication($container);
        $container->instance('files', new Filesystem());
        $container->instance('config', new Repository([
            'flowcatalyst' => [
                'client' => 'default-client',
                'definitions' => ['application_map' => []],
            ],
        ]));
    }

    protected function tearDown(): void
    {
        Container::setInstance(null);
        Facade::clearResolvedInstances();
        Facade::setFacadeApplication(null);
    }

    public function test_scanner_picks_up_as_connection_with_application_resolution(): void
    {
        $scanner = new DefinitionScanner();
        $definitions = $scanner->scan(
            [__DIR__ . '/../../Fixtures/Connections'],
            applicationCode: 'default-app',
        );

        $this->assertCount(2, $definitions->connections);

        $byCode = [];
        foreach ($definitions->connections as $row) {
            $byCode[$row['code']] = $row;
        }

        // No `application:` override, no namespace-map match → null; the
        // ultimate default (--app / FLOWCATALYST_APP_CODE) is applied later,
        // by whoever groups these rows (e.g. the sync command).
        $this->assertNull($byCode['orders-webhook']['_application']);
        // No `client:` override → the config default applies.
        $this->assertSame('default-client', $byCode['orders-webhook']['client']);
        $this->assertSame('Orders Webhook', $byCode['orders-webhook']['name']);

        // Explicit `application:`/`client:` on the attribute win.
        $this->assertSame('billing', $byCode['acme-webhook']['_application']);
        $this->assertSame('acme', $byCode['acme-webhook']['client']);

        // Routing fields never leak into the payload proper.
        $this->assertArrayNotHasKey('application', $byCode['orders-webhook']);
    }

    public function test_scanned_connections_round_trip_through_the_cache(): void
    {
        $definitions = new ScannedDefinitions(connections: [
            ['code' => 'orders-webhook', 'name' => 'Orders Webhook', '_class' => 'C', '_application' => 'orders', 'client' => null],
        ]);

        $restored = ScannedDefinitions::fromArray(json_decode(json_encode($definitions->toArray()), true));

        $this->assertSame($definitions->connections, $restored->connections);
        $this->assertFalse($restored->isEmpty());
        $this->assertSame(1, $restored->count());
    }
}
