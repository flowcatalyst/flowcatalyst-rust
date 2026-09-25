<?php

declare(strict_types=1);

namespace FlowCatalyst\Auth;

use FlowCatalyst\Auth\DTOs\FlowCatalystUser;
use Illuminate\Contracts\Auth\Access\Authorizable as AuthorizableContract;
use Illuminate\Contracts\Auth\Authenticatable as AuthenticatableContract;
use Illuminate\Foundation\Auth\Access\Authorizable;
use Illuminate\Support\Collection;

/**
 * An authenticated FlowCatalyst principal (a user OR a service account)
 * resolved from a Bearer access token. It implements Laravel's Authenticatable
 * + Authorizable contracts AND mirrors the spatie/laravel-permission method
 * surface, so a token-authenticated caller behaves exactly like a Spatie-
 * enabled Eloquent user in your controllers and policies:
 *
 *     if (! $auth->hasPermissionTo('platform:messaging:event:view')) abort(403);
 *     $auth->hasRole('integral:administrator');
 *     $auth->can('platform:messaging:event:view');   // Gate -> checkPermissionTo
 *
 * Roles come from the token's `roles` claim; permissions are resolved offline
 * from the registered RBAC catalogue (FlowCatalyst wildcards honoured). It is
 * READ-ONLY: Spatie write methods (assignRole, givePermissionTo, …) are not
 * supported and throw.
 *
 * Application/client scope (parity with Passport's app-identity middleware) is
 * exposed via getApplications()/hasApplicationAccess() and getClients()/
 * hasClientAccess()/hasFullAccess().
 */
final class FlowCatalystAuthenticatable implements AuthenticatableContract, AuthorizableContract
{
    use Authorizable;

    public function __construct(public readonly FlowCatalystUser $user)
    {
    }

    public function fcUser(): FlowCatalystUser
    {
        return $this->user;
    }

    // ── Authenticatable ─────────────────────────────────────────────────
    public function getAuthIdentifierName(): string
    {
        return 'sub';
    }

    public function getAuthIdentifier(): string
    {
        return $this->user->sub;
    }

    public function getAuthPassword(): string
    {
        return '';
    }

    public function getAuthPasswordName(): string
    {
        return 'password';
    }

    public function getRememberToken(): string
    {
        return '';
    }

    public function setRememberToken($value): void
    {
        // Stateless token auth — nothing to remember.
    }

    public function getRememberTokenName(): string
    {
        return '';
    }

    // ── Roles (Spatie surface) ──────────────────────────────────────────

    /**
     * True if the principal has ANY of the given role(s). Mirrors Spatie's
     * hasRole(string|array).
     *
     * @param string|iterable<int,string> $roles
     */
    public function hasRole($roles, ?string $guard = null): bool
    {
        foreach ($this->normalize($roles) as $role) {
            if ($this->user->hasRole($role)) {
                return true;
            }
        }
        return false;
    }

    public function hasAnyRole(...$roles): bool
    {
        return $this->hasRole($this->flatten($roles));
    }

    /** @param string|iterable<int,string> $roles */
    public function hasAllRoles($roles, ?string $guard = null): bool
    {
        foreach ($this->normalize($roles) as $role) {
            if (! $this->user->hasRole($role)) {
                return false;
            }
        }
        return true;
    }

    /** @param string|iterable<int,string> $roles */
    public function hasExactRoles($roles, ?string $guard = null): bool
    {
        $want = collect($this->normalize($roles))->sort()->values()->all();
        $have = $this->getRoleNames()->sort()->values()->all();
        return $want === $have;
    }

    public function getRoleNames(): Collection
    {
        return collect($this->user->getRoles());
    }

    // ── Permissions (Spatie surface) ────────────────────────────────────

    /**
     * True if the principal holds the given permission (FlowCatalyst wildcard
     * matching applies). Mirrors Spatie's hasPermissionTo(string) — but never
     * throws PermissionDoesNotExist; an unknown permission is simply false.
     *
     * @param string|\BackedEnum $permission
     */
    public function hasPermissionTo($permission, $guardName = null): bool
    {
        return $this->user->hasPermissionTo([$this->permissionName($permission)]);
    }

    /**
     * The entry point spatie/laravel-permission's Gate::before calls, so
     * $user->can(...) / @can / policies route here. Returns bool, never throws.
     *
     * @param string|\BackedEnum $permission
     */
    public function checkPermissionTo($permission, $guardName = null): bool
    {
        return $this->hasPermissionTo($permission, $guardName);
    }

    public function hasAnyPermission(...$permissions): bool
    {
        return $this->user->hasAnyPermissionTo($this->flatten($permissions));
    }

    public function hasAllPermissions(...$permissions): bool
    {
        foreach ($this->flatten($permissions) as $permission) {
            if (! $this->hasPermissionTo($permission)) {
                return false;
            }
        }
        return true;
    }

    /**
     * FlowCatalyst is pure RBAC — every permission is granted via a role, none
     * are assigned directly to the principal. So "direct" permissions are
     * always empty/false; use hasPermissionTo()/getAllPermissions() for the
     * effective set.
     */
    public function hasDirectPermission($permission): bool
    {
        return false;
    }

    public function getPermissionNames(): Collection
    {
        return collect($this->user->permissions);
    }

    public function getAllPermissions(): Collection
    {
        return $this->getPermissionNames();
    }

    public function getDirectPermissions(): Collection
    {
        // FlowCatalyst grants permissions only via roles — none are "direct".
        return collect();
    }

    // ── Application / client scope (parity with passport.app.identity) ───

    /** @return array<int,string> application codes this principal may act in */
    public function getApplications(): array
    {
        return $this->user->getApplications();
    }

    /**
     * True when the principal may reach `$applicationCode` — matched against
     * EITHER half of the `applications` claim's `"{id}:{code}"` pairs (the
     * caller may pass an id or a code), or when the principal reaches every
     * application ({@see \FlowCatalyst\Auth\DTOs\FlowCatalystUser::hasAllApplications()}
     * — the `"*"` entry or legacy `all_applications` flag), or when it holds
     * the platform-wide `clients` anchor ({@see hasFullAccess()}).
     */
    public function hasApplicationAccess(string $applicationCode): bool
    {
        if ($this->user->hasFullAccess() || $this->user->hasAllApplications()) {
            return true;
        }
        return in_array($applicationCode, $this->user->getApplications(), true)
            || in_array($applicationCode, $this->user->getApplicationCodes(), true);
    }

    /** @return array<int,string> client (tenant) ids/identifiers */
    public function getClients(): array
    {
        return $this->user->getClients();
    }

    public function hasClientAccess(string $clientId): bool
    {
        return $this->user->hasClientAccess($clientId);
    }

    public function hasFullAccess(): bool
    {
        return $this->user->hasFullAccess();
    }

    // ── Read-only guards (Spatie write methods are unsupported) ──────────
    public function assignRole(...$roles): never
    {
        throw new \BadMethodCallException('FlowCatalyst token identities are read-only; assign roles on the platform.');
    }

    public function removeRole(...$roles): never
    {
        throw new \BadMethodCallException('FlowCatalyst token identities are read-only; manage roles on the platform.');
    }

    public function givePermissionTo(...$permissions): never
    {
        throw new \BadMethodCallException('FlowCatalyst token identities are read-only; grant permissions on the platform.');
    }

    public function revokePermissionTo(...$permissions): never
    {
        throw new \BadMethodCallException('FlowCatalyst token identities are read-only; manage permissions on the platform.');
    }

    // ── helpers ─────────────────────────────────────────────────────────

    /**
     * @param string|iterable<int,string> $roles
     * @return array<int,string>
     */
    private function normalize($roles): array
    {
        if (is_string($roles)) {
            return [$roles];
        }
        return collect($roles)->map(fn ($r) => (string) $r)->all();
    }

    /**
     * @param array<int,mixed> $args
     * @return array<int,string>
     */
    private function flatten(array $args): array
    {
        return collect($args)->flatten()->map(fn ($v) => $this->permissionName($v))->all();
    }

    private function permissionName($value): string
    {
        if ($value instanceof \BackedEnum) {
            return (string) $value->value;
        }
        return (string) $value;
    }
}
