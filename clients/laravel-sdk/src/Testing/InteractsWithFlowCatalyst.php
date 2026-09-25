<?php

declare(strict_types=1);

namespace FlowCatalyst\Testing;

use FlowCatalyst\Auth\FlowCatalystAuthenticatable;

/**
 * Test-case trait: authenticate a FlowCatalyst principal for the duration of a
 * test. Use in a class that extends Laravel's TestCase (so `actingAs()` is
 * available).
 *
 *   use FlowCatalyst\Testing\InteractsWithFlowCatalyst;
 *
 *   $this->actingAsFlowCatalyst(['hr-manager'])->get('/dashboard')->assertOk();
 *
 * Because it goes through Laravel's `actingAs`, the guard's user is set
 * directly — no fake token, no JWT parsing.
 */
trait InteractsWithFlowCatalyst
{
    /**
     * Authenticate as the given principal, or build one from role names.
     *
     * @param FlowCatalystAuthenticatable|array<int, string> $principalOrRoles
     *        A ready principal, or a list of role names to build one from.
     */
    protected function actingAsFlowCatalyst(
        FlowCatalystAuthenticatable|array $principalOrRoles,
        string $guard = 'fc-session',
    ): static {
        $principal = is_array($principalOrRoles)
            ? FlowCatalystTestPrincipal::make(roles: $principalOrRoles)
            : $principalOrRoles;

        $this->actingAs($principal, $guard);

        return $this;
    }
}
