<?php

declare(strict_types=1);

namespace FlowCatalyst\Sync;

/**
 * Seeds a {@see SyncDefinitionSet}'s permissions and roles into the local
 * spatie/laravel-permission tables, so the app's authoritative role/permission
 * model is mirrored locally — not just pushed to the platform.
 *
 * Roles are stored under their FlowCatalyst (app-prefixed) name, e.g.
 * "example:editor", so they line up exactly with what the platform hands back
 * in the token on login. No-op unless spatie/laravel-permission is installed.
 */
final class SpatieSeeder
{
    public function __construct(private readonly string $guard = 'web') {}

    /**
     * Whether spatie/laravel-permission is installed.
     */
    public function isAvailable(): bool
    {
        return class_exists(\Spatie\Permission\Models\Permission::class)
            && class_exists(\Spatie\Permission\Models\Role::class);
    }

    /**
     * Mirror the definition set into Spatie.
     *
     * @return array{permissions: int, roles: int} counts seeded
     */
    public function seed(SyncDefinitionSet $definitions): array
    {
        if (!$this->isAvailable()) {
            return ['permissions' => 0, 'roles' => 0];
        }

        /** @var class-string $permissionModel */
        $permissionModel = config('permission.models.permission', \Spatie\Permission\Models\Permission::class);
        /** @var class-string $roleModel */
        $roleModel = config('permission.models.role', \Spatie\Permission\Models\Role::class);

        /** @var array<string, true> $seenPermissions distinct permission names */
        $seenPermissions = [];
        $ensure = function (string $name) use ($permissionModel, &$seenPermissions) {
            $permission = $permissionModel::findOrCreate($name, $this->guard);
            $seenPermissions[$name] = true;
            return $permission;
        };

        foreach ($definitions->getPermissions() as $entry) {
            $name = is_array($entry) ? ($entry['permission'] ?? null) : (string) $entry;
            if (is_string($name) && $name !== '') {
                $ensure($name);
            }
        }

        $roles = 0;
        foreach ($definitions->getRoles() as $role) {
            $name = $role['name'] ?? null;
            if (!is_string($name) || $name === '') {
                continue;
            }

            // Match the platform's app-prefixed role name (e.g. "example:editor").
            $fqName = $definitions->applicationCode . ':' . $name;

            $permissionObjects = [];
            foreach (($role['permissions'] ?? []) as $permName) {
                if (is_string($permName) && $permName !== '') {
                    $permissionObjects[] = $ensure($permName);
                }
            }

            // Authoritative for SDK-defined roles: the definition's permissions
            // become the role's set under this guard.
            $roleModel::findOrCreate($fqName, $this->guard)->syncPermissions($permissionObjects);
            $roles++;
        }

        if (class_exists(\Spatie\Permission\PermissionRegistrar::class) && function_exists('app')) {
            app(\Spatie\Permission\PermissionRegistrar::class)->forgetCachedPermissions();
        }

        return ['permissions' => count($seenPermissions), 'roles' => $roles];
    }
}
