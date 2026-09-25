<?php

declare(strict_types=1);

namespace FlowCatalyst\Tests\Unit\Auth;

use FlowCatalyst\Auth\DTOs\FlowCatalystUser;
use FlowCatalyst\Auth\FlowCatalystAuthenticatable;
use PHPUnit\Framework\TestCase;

final class FlowCatalystAuthenticatableTest extends TestCase
{
    private function make(array $roles, array $permissions, array $extraClaims = []): FlowCatalystAuthenticatable
    {
        $user = new FlowCatalystUser(
            sub: 'svc_123',
            email: null,
            name: 'CI Service Account',
            claims: array_merge(['roles' => $roles], $extraClaims),
            permissions: $permissions,
            mechanism: 'token',
        );
        return new FlowCatalystAuthenticatable($user);
    }

    public function test_authenticatable_identity(): void
    {
        $auth = $this->make(['integral:administrator'], []);
        $this->assertSame('svc_123', $auth->getAuthIdentifier());
        $this->assertSame('', $auth->getAuthPassword());
        $this->assertSame('', $auth->getRememberToken());
    }

    public function test_roles_spatie_surface(): void
    {
        $auth = $this->make(['integral:administrator', 'integral:viewer'], []);

        $this->assertTrue($auth->hasRole('integral:administrator'));
        $this->assertFalse($auth->hasRole('integral:ghost'));
        $this->assertTrue($auth->hasRole(['integral:ghost', 'integral:viewer']));      // any
        $this->assertTrue($auth->hasAnyRole('x', 'integral:viewer'));                  // variadic any
        $this->assertTrue($auth->hasAllRoles(['integral:administrator', 'integral:viewer']));
        $this->assertFalse($auth->hasAllRoles(['integral:administrator', 'integral:ghost']));
        $this->assertTrue($auth->hasExactRoles(['integral:viewer', 'integral:administrator']));
        $this->assertFalse($auth->hasExactRoles(['integral:administrator']));
        $this->assertEqualsCanonicalizing(
            ['integral:administrator', 'integral:viewer'],
            $auth->getRoleNames()->all(),
        );
    }

    public function test_permissions_spatie_surface_with_wildcards(): void
    {
        $auth = $this->make(
            ['integral:administrator'],
            ['platform:messaging:event:view', 'platform:messaging:dispatch-job:*'],
        );

        // exact
        $this->assertTrue($auth->hasPermissionTo('platform:messaging:event:view'));
        // wildcard
        $this->assertTrue($auth->hasPermissionTo('platform:messaging:dispatch-job:create'));
        $this->assertTrue($auth->checkPermissionTo('platform:messaging:dispatch-job:delete'));
        // not granted
        $this->assertFalse($auth->hasPermissionTo('platform:iam:user:view'));
        // never throws on unknown permission (unlike Spatie's PermissionDoesNotExist)
        $this->assertFalse($auth->hasPermissionTo('does:not:exist:here'));

        $this->assertTrue($auth->hasAnyPermission('platform:iam:user:view', 'platform:messaging:event:view'));
        $this->assertFalse($auth->hasAllPermissions('platform:messaging:event:view', 'platform:iam:user:view'));

        // FlowCatalyst is pure RBAC: every permission is role-derived, none direct.
        $this->assertFalse($auth->hasDirectPermission('platform:messaging:event:view'));
        $this->assertCount(0, $auth->getDirectPermissions());
        $this->assertEqualsCanonicalizing(
            ['platform:messaging:event:view', 'platform:messaging:dispatch-job:*'],
            $auth->getAllPermissions()->all(),
        );
    }

    public function test_application_and_client_scope(): void
    {
        // Real tokens carry "{id}:{code}" pairs (ClaimShapes), never bare
        // codes — a bare-code fixture here would mask a comparison against
        // the wrong half of the pair (see hasApplicationAccess()'s P5 fix).
        $auth = $this->make(
            ['integral:administrator'],
            [],
            ['applications' => ['app_01HXAPP:integral'], 'clients' => ['clt_abc']],
        );
        $this->assertTrue($auth->hasApplicationAccess('integral'));      // matches the code half
        $this->assertTrue($auth->hasApplicationAccess('app_01HXAPP'));   // matches the id half
        $this->assertFalse($auth->hasApplicationAccess('yard'));
        $this->assertFalse($auth->hasFullAccess());

        $full = $this->make(['integral:administrator'], [], ['clients' => ['*']]);
        $this->assertTrue($full->hasFullAccess());
        $this->assertTrue($full->hasClientAccess('anything'));
    }

    public function test_application_access_true_for_all_applications_without_clients_wildcard(): void
    {
        // hasAllApplications() ("*"/all_applications) must grant access on
        // its own — not only the unrelated `clients` anchor wildcard.
        $auth = $this->make(
            ['integral:administrator'],
            [],
            ['applications' => ['*'], 'clients' => ['clt_abc']],
        );
        $this->assertFalse($auth->hasFullAccess());
        $this->assertTrue($auth->hasApplicationAccess('anything-at-all'));
    }

    public function test_write_methods_are_read_only(): void
    {
        $auth = $this->make(['integral:administrator'], []);
        $this->expectException(\BadMethodCallException::class);
        $auth->assignRole('integral:superuser');
    }
}
