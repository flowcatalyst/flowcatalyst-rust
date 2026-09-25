<?php

declare(strict_types=1);

namespace FlowCatalyst\Tests\Unit\Testing;

use FlowCatalyst\Auth\Contracts\PermissionResolver;
use FlowCatalyst\Auth\FlowCatalystAuthenticatable;
use FlowCatalyst\Testing\FakePermissionResolver;
use FlowCatalyst\Testing\FlowCatalystTestPrincipal;
use Illuminate\Container\Container;
use PHPUnit\Framework\TestCase;

final class FlowCatalystTestPrincipalTest extends TestCase
{
    protected function tearDown(): void
    {
        Container::setInstance(null);
        parent::tearDown();
    }

    public function test_make_with_explicit_permissions_needs_no_container(): void
    {
        $principal = FlowCatalystTestPrincipal::make(
            roles: ['hr-manager'],
            permissions: ['hr:grading:record:view'],
        );

        $this->assertInstanceOf(FlowCatalystAuthenticatable::class, $principal);
        $this->assertSame(['hr-manager'], $principal->user->getRoles());
        $this->assertTrue($principal->user->hasPermissionTo(['hr:grading:record:view']));
        $this->assertFalse($principal->user->hasPermissionTo(['hr:grading:record:delete']));
    }

    public function test_make_resolves_permissions_from_bound_resolver(): void
    {
        $container = new Container();
        $container->instance(PermissionResolver::class, new FakePermissionResolver(byRole: [
            'hr-manager' => ['hr:grading:record:finalise'],
        ]));
        Container::setInstance($container);

        $principal = FlowCatalystTestPrincipal::make(roles: ['hr-manager']);

        $this->assertTrue($principal->user->hasPermissionTo(['hr:grading:record:finalise']));
    }

    public function test_make_sets_clients_and_applications_claims(): void
    {
        $principal = FlowCatalystTestPrincipal::make(
            roles: ['viewer'],
            permissions: [],
            clients: ['clt_1'],
            applications: ['app_hr'],
        );

        $this->assertSame(['clt_1'], $principal->user->claims['clients']);
        $this->assertSame(['app_hr'], $principal->user->claims['applications']);
    }

    public function test_fake_permission_resolver_flat_and_by_role(): void
    {
        $flat = new FakePermissionResolver(['platform:*:*:*']);
        $this->assertSame(['platform:*:*:*'], $flat->resolve(['anything']));

        $byRole = new FakePermissionResolver(byRole: [
            'a' => ['p1'],
            'b' => ['p2', 'p1'],
        ]);
        $this->assertEqualsCanonicalizing(['p1', 'p2'], $byRole->resolve(['a', 'b']));
        $this->assertSame([], $byRole->resolve(['ghost']));
    }
}
