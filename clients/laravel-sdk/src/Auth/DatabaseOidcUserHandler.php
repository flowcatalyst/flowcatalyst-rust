<?php

declare(strict_types=1);

namespace FlowCatalyst\Auth;

use FlowCatalyst\Auth\DTOs\FlowCatalystUser;
use Illuminate\Contracts\Auth\Authenticatable;
use Illuminate\Support\Facades\Auth;
use Illuminate\Support\Facades\Log;
use Illuminate\Support\Facades\Schema;
use Illuminate\Support\Str;

/**
 * OIDC handler that bridges a FlowCatalyst identity into the application's
 * NATIVE Laravel auth — so stock `->middleware('auth')`, `Auth::user()`,
 * `@auth`, and policies all recognise the OIDC user with no extra wiring.
 *
 * This is the friction-free default for a new app that wants FlowCatalyst as
 * its OIDC provider. On a successful OIDC callback it:
 *
 *   1. keeps the SDK session principal (via {@see DefaultOidcUserHandler}) so
 *      the `fc.*` middleware and {@see FlowCatalystUser} helpers keep working;
 *   2. upserts a local user record (model from `flowcatalyst.oidc.user_model`,
 *      default `App\Models\User`), matched by email;
 *   3. optionally syncs the FlowCatalyst `roles` claim into Spatie roles — only
 *      when spatie/laravel-permission is installed AND the user model uses the
 *      `HasRoles` trait AND `flowcatalyst.oidc.sync_roles` is enabled;
 *   4. logs the user into the default Laravel guard via {@see Auth::login()}.
 *
 * Selected as the SDK default when `flowcatalyst.oidc.handler === 'database'`.
 * Any app that binds its own {@see \FlowCatalyst\Auth\Contracts\OidcUserHandler}
 * (e.g. for tenant checks or extra columns) overrides this — the SDK only binds
 * a default when the contract is not already bound.
 *
 * Designed to degrade gracefully: if the configured model is missing, the
 * identity has no email, or the upsert fails, it logs and falls back to the
 * session-only behaviour of the parent rather than 500-ing mid-login.
 */
class DatabaseOidcUserHandler extends DefaultOidcUserHandler
{
    /**
     * Upsert + log the FlowCatalyst user into the native Laravel guard, while
     * preserving the SDK session principal the rest of the SDK relies on.
     */
    public function handleAuthenticatedUser(FlowCatalystUser $fcUser): mixed
    {
        // Keep the session principal so the fc.* middleware + helpers still work.
        parent::handleAuthenticatedUser($fcUser);

        $user = $this->resolveLocalUser($fcUser);
        if ($user === null) {
            // No usable local model — behave like the session-only default.
            return $fcUser;
        }

        $this->syncRoles($user, $fcUser->getRoles());

        Auth::login($user, remember: (bool) config('flowcatalyst.oidc.remember_login', false));

        return $user;
    }

    /**
     * Tear down the native session login first, then the SDK session blob.
     */
    public function handleLogout(): void
    {
        if (Auth::check()) {
            Auth::logout();
        }

        parent::handleLogout();
    }

    /**
     * Upsert and return the local user for this FlowCatalyst identity, or null
     * when no usable user model is configured/available.
     */
    protected function resolveLocalUser(FlowCatalystUser $fcUser): ?Authenticatable
    {
        $modelClass = ltrim((string) config('flowcatalyst.oidc.user_model', \App\Models\User::class), '\\');

        if ($modelClass === '' || !class_exists($modelClass)) {
            Log::warning('FlowCatalyst OIDC: configured user_model not found; skipping native login', [
                'user_model' => $modelClass,
            ]);

            return null;
        }

        if ($fcUser->email === null || $fcUser->email === '') {
            Log::warning('FlowCatalyst OIDC: identity has no email; skipping native login', [
                'sub' => $fcUser->sub,
            ]);

            return null;
        }

        try {
            /** @var \Illuminate\Database\Eloquent\Model $probe */
            $probe = new $modelClass();
            $user = $probe->newQuery()->where('email', $fcUser->email)->first();

            if ($user === null) {
                $user = new $modelClass();
                $user->email = $fcUser->email;
                if ($this->hasColumn($probe, 'name')) {
                    $user->name = $fcUser->name ?? $fcUser->email;
                }
                // Random, unusable password for OIDC-provisioned accounts (when
                // the table has the column). They never sign in with a password.
                if ($this->hasColumn($probe, 'password')) {
                    $user->password = bcrypt(Str::random(40));
                }
                $user->save();
            } elseif ($fcUser->name && $this->hasColumn($probe, 'name') && $user->name !== $fcUser->name) {
                $user->name = $fcUser->name;
                $user->save();
            }

            return $user instanceof Authenticatable ? $user : null;
        } catch (\Throwable $e) {
            Log::error('FlowCatalyst OIDC: failed to upsert local user', [
                'email' => $fcUser->email,
                'error' => $e->getMessage(),
            ]);

            return null;
        }
    }

    /**
     * Sync the FlowCatalyst `roles` claim into the user's Spatie roles.
     *
     * No-op unless spatie/laravel-permission is installed, the user model uses
     * the `HasRoles` trait, and `flowcatalyst.oidc.sync_roles` is enabled. Role
     * resolution / assignment failures are logged but never block the login.
     *
     * @param array<int, string> $fcRoles
     */
    protected function syncRoles(Authenticatable $user, array $fcRoles): void
    {
        if (!config('flowcatalyst.oidc.sync_roles', true) || $fcRoles === []) {
            return;
        }

        $hasRolesTrait = \Spatie\Permission\Traits\HasRoles::class;
        if (!trait_exists($hasRolesTrait) || !in_array($hasRolesTrait, class_uses_recursive($user), true)) {
            return;
        }

        $guard = (string) config('flowcatalyst.oidc.roles_guard', 'web');
        $mode = (string) config('flowcatalyst.oidc.sync_roles_mode', 'additive');
        $createMissing = (bool) config('flowcatalyst.oidc.create_missing_roles', false);

        try {
            /** @var class-string $roleClass */
            $roleClass = config('permission.models.role', \Spatie\Permission\Models\Role::class);

            /** @var array<string, object> $resolved roleName => Spatie role model */
            $resolved = [];
            foreach (array_unique($fcRoles) as $name) {
                $role = $roleClass::query()->where('name', $name)->where('guard_name', $guard)->first();
                if ($role === null && $createMissing) {
                    $role = $roleClass::create(['name' => $name, 'guard_name' => $guard]);
                }
                if ($role !== null) {
                    $resolved[$name] = $role;
                }
            }

            if ($resolved === []) {
                return;
            }

            $roles = array_values($resolved);

            if ($mode === 'replace') {
                // Authoritative: the token's roles become the user's full set.
                $user->syncRoles($roles);
            } else {
                // Additive (default): grant new roles, never remove existing ones.
                foreach ($roles as $role) {
                    if (!$user->hasRole($role)) {
                        $user->assignRole($role);
                    }
                }
            }

            // Mirror each role's permissions from FlowCatalyst into the local
            // Spatie role, so $user->can('app:context:aggregate:action') resolves
            // locally — not just the role name. Best-effort (needs a service
            // account); skips silently if unavailable.
            if (config('flowcatalyst.oidc.sync_role_permissions', true)) {
                foreach ($resolved as $name => $role) {
                    $this->mirrorRolePermissions($name, $role, $guard);
                }
            }

            if (method_exists($user, 'forgetCachedPermissions')) {
                $user->forgetCachedPermissions();
            }
        } catch (\Throwable $e) {
            Log::error('FlowCatalyst OIDC: role sync failed; continuing login', [
                'roles' => $fcRoles,
                'guard' => $guard,
                'error' => $e->getMessage(),
            ]);
        }
    }

    /**
     * Mirror a FlowCatalyst role's permissions onto the local Spatie role, so
     * `$user->can(...)` works against FlowCatalyst-sourced permissions.
     * Authoritative for the role; best-effort and never blocks login.
     */
    protected function mirrorRolePermissions(string $roleName, object $role, string $guard): void
    {
        $permissions = $this->fetchRolePermissions($roleName);
        if ($permissions === null) {
            return; // platform unreachable / no service account — leave as-is.
        }

        try {
            /** @var class-string $permissionModel */
            $permissionModel = config('permission.models.permission', \Spatie\Permission\Models\Permission::class);

            $objects = [];
            foreach ($permissions as $permission) {
                if (is_string($permission) && $permission !== '') {
                    $objects[] = $permissionModel::findOrCreate($permission, $guard);
                }
            }

            if (method_exists($role, 'syncPermissions')) {
                $role->syncPermissions($objects);
            }
        } catch (\Throwable $e) {
            Log::warning('FlowCatalyst OIDC: failed to mirror role permissions; continuing', [
                'role' => $roleName,
                'error' => $e->getMessage(),
            ]);
        }
    }

    /**
     * Fetch a role's permission codes from FlowCatalyst (GET /api/roles/{name}),
     * cached by role name. Returns null when unreachable / no service account.
     *
     * @return array<int, string>|null
     */
    protected function fetchRolePermissions(string $roleName): ?array
    {
        $ttl = (int) config('flowcatalyst.oidc.role_permissions_cache_ttl', 300);
        $key = 'fc.role.perms.' . md5($roleName);

        $cached = \Illuminate\Support\Facades\Cache::get($key);
        if (is_array($cached)) {
            return $cached;
        }

        try {
            $role = app(\FlowCatalyst\Client\FlowCatalystClient::class)->roles()->get($roleName);
            $permissions = array_values(array_filter($role->permissions, 'is_string'));
        } catch (\Throwable $e) {
            Log::warning('FlowCatalyst OIDC: could not fetch role permissions', [
                'role' => $roleName,
                'error' => $e->getMessage(),
            ]);

            return null;
        }

        \Illuminate\Support\Facades\Cache::put($key, $permissions, $ttl);

        return $permissions;
    }

    /**
     * Whether the model's table has the given column. Wrapped so a schema-
     * inspection hiccup degrades to "skip that column" instead of failing login.
     */
    private function hasColumn(object $model, string $column): bool
    {
        try {
            /** @var \Illuminate\Database\Eloquent\Model $model */
            return Schema::connection($model->getConnectionName())
                ->hasColumn($model->getTable(), $column);
        } catch (\Throwable) {
            return false;
        }
    }
}
