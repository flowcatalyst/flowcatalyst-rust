<?php

declare(strict_types=1);

namespace FlowCatalyst\Testing;

use FlowCatalyst\Auth\Contracts\PermissionResolver;
use FlowCatalyst\Auth\DTOs\FlowCatalystUser;
use FlowCatalyst\Auth\FlowCatalystAuthenticatable;

/**
 * Factory for a fully-formed FlowCatalyst principal in tests — no real
 * OAuth/OIDC login, no access token, no network.
 *
 * Pair it with {@see InteractsWithFlowCatalyst::actingAsFlowCatalyst()} (or a
 * plain `$this->actingAs($principal, 'fc-session')`), which sets the guard's
 * user directly and so bypasses JWT parsing entirely.
 *
 * Permissions are either passed explicitly or resolved offline from the role
 * names via the bound {@see PermissionResolver} (your app's RbacCatalogue), so
 * `hasPermissionTo()` / `can()` / policies work with zero network.
 */
final class FlowCatalystTestPrincipal
{
    /**
     * @param array<int, string>       $roles        Role names EXACTLY as your
     *        RBAC catalogue keys them (bare, e.g. "hr-manager", for most apps —
     *        this is the same string the platform puts in the token's `roles`
     *        claim). A name the catalogue doesn't know resolves to zero
     *        permissions, silently — the same rule the real token follows.
     * @param array<int, string>|null  $permissions  Effective permission
     *        strings. When null (default) they are resolved from $roles via the
     *        bound PermissionResolver; pass an explicit list to skip the
     *        catalogue entirely (handy with {@see FakePermissionResolver}).
     * @param array<int, string>       $clients      Optional `clients` claim.
     * @param array<int, string>       $applications Optional `applications` claim.
     */
    public static function make(
        array $roles = [],
        ?array $permissions = null,
        ?string $sub = null,
        ?string $email = null,
        ?string $name = null,
        array $clients = [],
        array $applications = [],
    ): FlowCatalystAuthenticatable {
        $perms = $permissions ?? app(PermissionResolver::class)->resolve($roles);

        $slug = $roles !== [] ? implode('_', $roles) : 'none';

        $claims = ['roles' => array_values($roles)];
        if ($clients !== []) {
            $claims['clients'] = array_values($clients);
        }
        if ($applications !== []) {
            $claims['applications'] = array_values($applications);
        }

        $user = (new FlowCatalystUser(
            sub: $sub ?? 'prn_test_' . $slug,
            email: $email ?? $slug . '@example.test',
            name: $name ?? 'Test ' . $slug,
            claims: $claims,
        ))->withPermissions(array_values($perms));

        return new FlowCatalystAuthenticatable($user);
    }
}
