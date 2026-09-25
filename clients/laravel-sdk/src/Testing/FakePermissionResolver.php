<?php

declare(strict_types=1);

namespace FlowCatalyst\Testing;

use FlowCatalyst\Auth\Contracts\PermissionResolver;

/**
 * A {@see PermissionResolver} for tests. Use it when the app under test has no
 * offline RbacCatalogue bound but you still want `hasPermissionTo()` / `can()`
 * to work — bind it in your test's setUp:
 *
 *   $this->app->instance(
 *       PermissionResolver::class,
 *       new FakePermissionResolver(['platform:*:*:*']),   // grants everything
 *   );
 *
 * Or map specific roles to permissions:
 *
 *   new FakePermissionResolver(byRole: [
 *       'hr-manager' => ['hr:grading:record:view', 'hr:grading:record:finalise'],
 *   ]);
 */
final class FakePermissionResolver implements PermissionResolver
{
    /**
     * @param array<int, string>                    $permissions Returned for
     *        every role set (a flat grant), unioned with any $byRole matches.
     * @param array<string, array<int, string>>     $byRole      Per-role
     *        permission lists; each requested role contributes its own.
     */
    public function __construct(
        private readonly array $permissions = [],
        private readonly array $byRole = [],
    ) {
    }

    /**
     * @param  array<int, string> $roles
     * @return array<int, string>
     */
    public function resolve(array $roles, ?string $token = null): array
    {
        $out = $this->permissions;

        foreach ($roles as $role) {
            foreach ($this->byRole[$role] ?? [] as $permission) {
                $out[] = $permission;
            }
        }

        return array_values(array_unique($out));
    }
}
