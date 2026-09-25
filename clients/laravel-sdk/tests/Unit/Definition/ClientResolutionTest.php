<?php

declare(strict_types=1);

namespace FlowCatalyst\Tests\Unit\Definition;

use FlowCatalyst\Attributes\AsConnection;
use FlowCatalyst\Attributes\AsSubscription;
use FlowCatalyst\Definition\DefinitionScanner;
use Illuminate\Config\Repository;
use Illuminate\Container\Container;
use PHPUnit\Framework\Attributes\DataProvider;
use PHPUnit\Framework\TestCase;
use ReflectionMethod;

/**
 * Client resolution for #[AsSubscription] / #[AsConnection] (single-tenant
 * apps): an explicit `client:` on the attribute beats the config default
 * `flowcatalyst.client`; the config default applies when the attribute has
 * none; null when neither is set (a global definition). Unlike application
 * resolution there is no namespace map — a multi-tenant codebase builds one
 * `SyncDefinitionSet` per client instead (see SyncDefinitionSet::forClient()).
 */
final class ClientResolutionTest extends TestCase
{
    protected function tearDown(): void
    {
        Container::setInstance(null);
    }

    private function withConfigDefault(?string $default): void
    {
        $container = new Container();
        Container::setInstance($container);
        $container->instance('config', new Repository([
            'flowcatalyst' => ['client' => $default],
        ]));
    }

    private function resolve(object $instance): ?string
    {
        $m = new ReflectionMethod(DefinitionScanner::class, 'resolveClient');
        $m->setAccessible(true);
        return $m->invoke(new DefinitionScanner(), $instance);
    }

    /**
     * @return iterable<string, array{0: string}>
     */
    public static function attributeClasses(): iterable
    {
        yield 'subscription' => [AsSubscription::class];
        yield 'connection' => [AsConnection::class];
    }

    #[DataProvider('attributeClasses')]
    public function test_attribute_client_beats_config_default(string $class): void
    {
        $this->withConfigDefault('config-default');
        $instance = $this->makeWithClient($class, 'acme');

        $this->assertSame('acme', $this->resolve($instance));
    }

    #[DataProvider('attributeClasses')]
    public function test_config_default_applies_when_attribute_has_none(string $class): void
    {
        $this->withConfigDefault('config-default');
        $instance = $this->makeWithClient($class, null);

        $this->assertSame('config-default', $this->resolve($instance));
    }

    #[DataProvider('attributeClasses')]
    public function test_null_when_neither_is_set(string $class): void
    {
        $this->withConfigDefault(null);
        $instance = $this->makeWithClient($class, null);

        $this->assertNull($this->resolve($instance));
    }

    private function makeWithClient(string $class, ?string $client): object
    {
        return match ($class) {
            AsSubscription::class => new AsSubscription(
                code: 'c', name: 'N', target: '/hook', connectionCode: 'conn',
                queue: 'q', dispatchPoolCode: 'default', client: $client,
            ),
            AsConnection::class => new AsConnection(code: 'c', name: 'N', client: $client),
        };
    }
}
