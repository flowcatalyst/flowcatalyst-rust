<?php

declare(strict_types=1);

namespace FlowCatalyst\Tests\Unit\Auth;

use FlowCatalyst\Auth\Contracts\PermissionResolver;
use FlowCatalyst\Auth\Rbac\CataloguePermissionResolver;
use FlowCatalyst\Auth\Rbac\RbacBuilder;
use PHPUnit\Framework\TestCase;

final class CataloguePermissionResolverTest extends TestCase
{
    public function test_resolves_roles_to_permissions_and_ignores_token(): void
    {
        $catalogue = RbacBuilder::make()
            ->role('integral:administrator')->grants(['platform:messaging:*:*'])
            ->role('integral:viewer')->grants(['orders:view'])
            ->build();

        $resolver = new CataloguePermissionResolver($catalogue);
        $this->assertInstanceOf(PermissionResolver::class, $resolver);

        // Passed roles → permissions; token is ignored by the offline resolver.
        $this->assertSame(['orders:view'], $resolver->resolve(['integral:viewer'], 'ignored'));
        $this->assertEqualsCanonicalizing(
            ['platform:messaging:*:*', 'orders:view'],
            $resolver->resolve(['integral:administrator', 'integral:viewer']),
        );
        $this->assertSame([], $resolver->resolve(['ghost']));
    }
}
