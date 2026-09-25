<?php

declare(strict_types=1);

namespace FlowCatalyst\Tests\Unit\Auth\DTOs;

use FlowCatalyst\Auth\DTOs\FlowCatalystUser;
use FlowCatalyst\Auth\Rbac\RbacBuilder;
use PHPUnit\Framework\TestCase;

final class FlowCatalystUserTest extends TestCase
{
    /**
     * @param array<int, string> $roles
     * @param array<int, string> $clients
     */
    private function principal(array $roles = ['billing-admin'], array $clients = ['clt_a'], string $scope = 'CLIENT'): FlowCatalystUser
    {
        return FlowCatalystUser::fromAccessTokenClaims([
            'sub' => 'prn_x',
            'name' => 'Tester',
            'email' => 'test@example.com',
            'scope' => $scope,
            'clients' => $clients,
            'roles' => $roles,
            'applications' => ['billing'],
        ], accessToken: 'tok', mechanism: 'bearer');
    }

    public function test_has_role(): void
    {
        $p = $this->principal(roles: ['a', 'b', 'c']);
        $this->assertTrue($p->hasRole('a'));
        $this->assertFalse($p->hasRole('z'));
    }

    public function test_has_roles_is_all(): void
    {
        $p = $this->principal(roles: ['a', 'b']);
        $this->assertTrue($p->hasRoles(['a', 'b']));
        $this->assertFalse($p->hasRoles(['a', 'z']));
    }

    public function test_has_any_role_is_any(): void
    {
        $p = $this->principal(roles: ['a']);
        $this->assertTrue($p->hasAnyRole(['x', 'a']));
        $this->assertFalse($p->hasAnyRole(['x', 'y']));
    }

    public function test_permissions_resolve_via_with_rbac(): void
    {
        $rbac = RbacBuilder::make()
            ->role('billing-admin')->grants(['invoice:create', 'invoice:read'])
            ->build();
        $p = $this->principal()->withRbac($rbac);
        $this->assertTrue($p->hasPermissionTo(['invoice:read']));
        $this->assertTrue($p->hasPermissionTo(['invoice:create', 'invoice:read']));
        $this->assertFalse($p->hasPermissionTo(['invoice:void']));
        $this->assertTrue($p->hasAnyPermissionTo(['invoice:void', 'invoice:read']));
    }

    public function test_permission_wildcards(): void
    {
        $rbac = RbacBuilder::make()
            ->role('admin')->grants(['billing:*'])
            ->build();
        $p = $this->principal(roles: ['admin'])->withRbac($rbac);
        $this->assertTrue($p->hasPermissionTo(['billing:read']));
        $this->assertTrue($p->hasPermissionTo(['billing:invoice:export']));
        $this->assertFalse($p->hasPermissionTo(['ticket:read']));
    }

    public function test_anchor_scope_grants_implicit_cross_client_access(): void
    {
        $p = $this->principal(clients: ['*'], scope: 'ANCHOR');
        $this->assertTrue($p->isAnchor());
        $this->assertTrue($p->hasClientAccess('clt_anything'));
    }

    public function test_non_anchor_only_sees_own_clients(): void
    {
        $p = $this->principal(clients: ['clt_a', 'clt_b']);
        $this->assertFalse($p->isAnchor());
        $this->assertTrue($p->hasClientAccess('clt_a'));
        $this->assertFalse($p->hasClientAccess('clt_x'));
    }

    public function test_permission_set_empty_without_rbac(): void
    {
        $p = $this->principal();
        $this->assertFalse($p->hasPermissionTo(['anything']));
    }

    public function test_with_rbac_returns_a_new_instance(): void
    {
        $rbac = RbacBuilder::make()->role('billing-admin')->grants(['x'])->build();
        $original = $this->principal();
        $derived = $original->withRbac($rbac);
        $this->assertNotSame($original, $derived);
        $this->assertSame([], $original->permissions);
        $this->assertSame(['x'], $derived->permissions);
    }

    public function test_with_mechanism_returns_a_new_instance(): void
    {
        $p = $this->principal();
        $session = $p->withMechanism('session');
        $this->assertTrue($session->isSession());
        $this->assertFalse($session->isBearer());
    }

    public function test_service_account_has_no_email(): void
    {
        $p = FlowCatalystUser::fromAccessTokenClaims([
            'sub' => 'prn_svc',
            'name' => 'Service',
            'scope' => 'CLIENT',
            'clients' => ['clt_a'],
            'roles' => ['svc'],
        ], mechanism: 'bearer');
        $this->assertNull($p->email);
        $this->assertSame('prn_svc', $p->sub);
    }

    /**
     * The `applications` claim mirrors `clients`: "{id}:{code}" pairs, or the
     * single "*" sentinel. Apps want ids, codes and the flag separately.
     *
     * @param array<int, string> $applications
     */
    private function withApplications(array $applications, ?bool $allApplications = null): FlowCatalystUser
    {
        $claims = [
            'sub' => 'prn_x',
            'name' => 'Tester',
            'clients' => ['clt_a'],
            'roles' => [],
            'applications' => $applications,
        ];
        if ($allApplications !== null) {
            $claims['all_applications'] = $allApplications;
        }

        return FlowCatalystUser::fromAccessTokenClaims($claims);
    }

    public function test_it_splits_application_id_code_pairs(): void
    {
        $user = $this->withApplications(['app_1:alpha', 'app_2:beta']);

        $this->assertSame(['app_1', 'app_2'], $user->getApplications());
        $this->assertSame(['alpha', 'beta'], $user->getApplicationCodes());
        $this->assertFalse($user->hasAllApplications());
    }

    public function test_it_reads_the_wildcard_as_all_applications(): void
    {
        $user = $this->withApplications(['*']);

        $this->assertTrue($user->hasAllApplications());
        $this->assertSame([], $user->getApplications(), 'no restriction to enumerate');
    }

    public function test_it_still_accepts_bare_ids_from_a_pre_upgrade_token(): void
    {
        $user = $this->withApplications(['app_1', 'app_2']);

        $this->assertSame(['app_1', 'app_2'], $user->getApplications());
        $this->assertSame([], $user->getApplicationCodes());
    }

    public function test_it_honours_the_deprecated_all_applications_boolean(): void
    {
        $user = $this->withApplications([], true);

        $this->assertTrue($user->hasAllApplications());
    }

    public function test_it_keeps_a_colon_inside_an_application_code(): void
    {
        $user = $this->withApplications(['app_1:a:b']);

        $this->assertSame(['app_1'], $user->getApplications());
        $this->assertSame(['a:b'], $user->getApplicationCodes());
    }
}
