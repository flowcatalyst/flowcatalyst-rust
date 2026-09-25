<?php

declare(strict_types=1);

namespace FlowCatalyst\Attributes;

use Attribute;
use FlowCatalyst\DTOs\PermissionInput;

/**
 * Marks a class as a role definition for FlowCatalyst.
 *
 * Usage:
 * ```php
 * use FlowCatalyst\DTOs\PermissionInput;
 *
 * #[AsRole(
 *     name: 'admin',
 *     displayName: 'Administrator',
 *     description: 'Full administrative access',
 *     permissions: [
 *         new PermissionInput('myapp', 'users', 'user', 'view'),
 *         new PermissionInput('myapp', 'users', 'user', 'create'),
 *         new PermissionInput('myapp', 'settings', 'config', 'manage'),
 *     ],
 *     clientManaged: false
 * )]
 * class AdminRole {}
 * ```
 *
 * The role name will be prefixed with your application code when synced.
 * For example, if your app code is "myapp", the role becomes "myapp:admin".
 * Set `application:` to override it per definition (see the constructor).
 */
#[Attribute(Attribute::TARGET_CLASS)]
final class AsRole
{
    /**
     * Role name format: lowercase alphanumeric with hyphens/underscores, cannot start/end with hyphen or underscore.
     */
    private const NAME_PATTERN = '/^[a-z0-9]([a-z0-9_-]*[a-z0-9])?$/';

    /**
     * @param string $name Role name (will be prefixed with app code)
     * @param string|null $displayName Human-friendly display name
     * @param string|null $description Role description
     * @param array<PermissionInput|class-string|string> $permissions Structured
     *        permissions, #[AsPermission] class-strings (e.g. ViewPosts::class),
     *        or literal "app:context:aggregate:action" strings.
     * @param bool $clientManaged Whether clients can assign this role
     * @param string|null $application Application code this role belongs to.
     *        Overrides the global `application_code` / `application_map` during
     *        sync — set it when one codebase defines roles for more than one
     *        application. Also resolves the #[AsPermission] class-strings in
     *        `permissions`. Null = resolve from the namespace map / default.
     */
    public function __construct(
        public readonly string $name,
        public readonly ?string $displayName = null,
        public readonly ?string $description = null,
        public readonly array $permissions = [],
        public readonly bool $clientManaged = false,
        public readonly ?string $application = null,
    ) {}

    /**
     * Validate the role name.
     *
     * @return string|null Error message if invalid, null if valid
     */
    public function validate(): ?string
    {
        if (empty($this->name)) {
            return 'Role name cannot be empty';
        }

        if (str_contains($this->name, ':')) {
            return "Role name cannot contain colons (the app code will be prefixed automatically): {$this->name}";
        }

        if (!preg_match(self::NAME_PATTERN, $this->name)) {
            return "Role name must be lowercase alphanumeric with hyphens/underscores (cannot start/end with hyphen or underscore): {$this->name}";
        }

        return null;
    }

    /**
     * Check if this role definition is valid.
     */
    public function isValid(): bool
    {
        return $this->validate() === null;
    }

    /**
     * Convert to array format for API sync.
     *
     * @param string|null $defaultApplication App code used to resolve
     *        #[AsPermission] class-strings (defaults to the configured app code).
     */
    public function toArray(?string $defaultApplication = null): array
    {
        $data = [
            'name' => $this->name,
        ];

        if ($this->displayName !== null) {
            $data['displayName'] = $this->displayName;
        }

        if ($this->description !== null) {
            $data['description'] = $this->description;
        }

        $permissions = $this->permissionStrings($defaultApplication);
        if (!empty($permissions)) {
            $data['permissions'] = $permissions;
        }

        if ($this->clientManaged) {
            $data['clientManaged'] = true;
        }

        return $data;
    }

    /**
     * Resolve this role's permissions to a flat list of
     * "app:context:aggregate:action" strings (the shape the roles-sync API
     * expects). Each entry may be a {@see PermissionInput}, an #[AsPermission]
     * class-string, or a literal permission string.
     *
     * @return array<int, string>
     */
    public function permissionStrings(?string $defaultApplication = null): array
    {
        $out = [];

        foreach ($this->permissions as $permission) {
            if ($permission instanceof PermissionInput) {
                $out[] = $permission->toPermissionString();
                continue;
            }

            if (is_string($permission)) {
                $out[] = class_exists($permission)
                    ? self::resolvePermissionClass($permission, $defaultApplication)
                    : strtolower($permission);
            }
        }

        return array_values(array_unique($out));
    }

    /**
     * Resolve an #[AsPermission] class-string to its permission string.
     */
    private static function resolvePermissionClass(string $class, ?string $defaultApplication): string
    {
        $attributes = (new \ReflectionClass($class))->getAttributes(AsPermission::class);
        if ($attributes === []) {
            throw new \InvalidArgumentException(
                "Role '{$class}' is referenced as a permission but is missing the #[AsPermission] attribute."
            );
        }

        /** @var AsPermission $permission */
        $permission = $attributes[0]->newInstance();

        return $permission->toPermissionString($defaultApplication);
    }
}
