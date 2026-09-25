<?php

declare(strict_types=1);

namespace FlowCatalyst\Sync;

use FlowCatalyst\DTOs\PermissionInput;

/**
 * Represents a role definition for syncing to FlowCatalyst.
 *
 * Can be used directly or converted to array for the sync API.
 *
 * Example:
 * ```php
 * $role = RoleDefinition::make('admin')
 *     ->withDisplayName('Administrator')
 *     ->withPermissions([
 *         PermissionInput::make('myapp', 'users', 'user', 'view'),
 *         PermissionInput::make('myapp', 'users', 'user', 'create'),
 *     ]);
 * ```
 */
final class RoleDefinition
{
    /**
     * Role name format: lowercase alphanumeric with hyphens/underscores, cannot start/end with hyphen or underscore.
     */
    private const NAME_PATTERN = '/^[a-z0-9]([a-z0-9_-]*[a-z0-9])?$/';

    /**
     * @param string $name Unique role identifier (e.g., 'admin', 'editor')
     * @param string|null $displayName Human-readable name
     * @param string|null $description Role description
     * @param PermissionInput[] $permissions List of structured permissions this role grants
     * @param bool $clientManaged Whether clients can assign this role to their users
     */
    public function __construct(
        public readonly string $name,
        public readonly ?string $displayName = null,
        public readonly ?string $description = null,
        public readonly array $permissions = [],
        public readonly bool $clientManaged = false,
    ) {}

    /**
     * Create a new role definition with fluent syntax.
     */
    public static function make(string $name): self
    {
        return new self($name);
    }

    /**
     * Validate the role name.
     *
     * @return string|null Error message if invalid, null if valid
     */
    public function validate(): ?string
    {
        return self::validateName($this->name);
    }

    /**
     * Check if this role definition is valid.
     */
    public function isValid(): bool
    {
        return $this->validate() === null;
    }

    /**
     * Validate a role name.
     *
     * Rules:
     * - Lowercase alphanumeric with hyphens and underscores only
     * - Cannot start or end with a hyphen or underscore
     * - Cannot contain colons (reserved for app prefix)
     * - At least 1 character
     *
     * @return string|null Error message if invalid, null if valid
     */
    public static function validateName(string $name): ?string
    {
        if (empty($name)) {
            return 'Role name cannot be empty';
        }

        if (str_contains($name, ':')) {
            return "Role name cannot contain colons (the app code will be prefixed automatically): {$name}";
        }

        if (!preg_match(self::NAME_PATTERN, $name)) {
            return "Role name must be lowercase alphanumeric with hyphens/underscores (cannot start/end with hyphen or underscore): {$name}";
        }

        return null;
    }

    /**
     * Create a copy with a different display name.
     */
    public function withDisplayName(string $displayName): self
    {
        return new self(
            name: $this->name,
            displayName: $displayName,
            description: $this->description,
            permissions: $this->permissions,
            clientManaged: $this->clientManaged,
        );
    }

    /**
     * Create a copy with a different description.
     */
    public function withDescription(string $description): self
    {
        return new self(
            name: $this->name,
            displayName: $this->displayName,
            description: $description,
            permissions: $this->permissions,
            clientManaged: $this->clientManaged,
        );
    }

    /**
     * Create a copy with different permissions.
     *
     * @param PermissionInput[] $permissions
     */
    public function withPermissions(array $permissions): self
    {
        return new self(
            name: $this->name,
            displayName: $this->displayName,
            description: $this->description,
            permissions: $permissions,
            clientManaged: $this->clientManaged,
        );
    }

    /**
     * Create a copy that is client-managed.
     */
    public function clientManaged(bool $clientManaged = true): self
    {
        return new self(
            name: $this->name,
            displayName: $this->displayName,
            description: $this->description,
            permissions: $this->permissions,
            clientManaged: $clientManaged,
        );
    }

    /**
     * Convert to array for the sync API.
     *
     * @return array<string, mixed>
     */
    public function toArray(): array
    {
        $data = ['name' => $this->name];

        if ($this->displayName !== null) {
            $data['displayName'] = $this->displayName;
        }

        if ($this->description !== null) {
            $data['description'] = $this->description;
        }

        if (!empty($this->permissions)) {
            // The roles-sync API expects a flat list of
            // "app:context:aggregate:action" strings (not objects).
            // toPermissionString() produces exactly that shape.
            $data['permissions'] = array_map(
                fn(PermissionInput $p) => $p->toPermissionString(),
                $this->permissions
            );
        }

        if ($this->clientManaged) {
            $data['clientManaged'] = true;
        }

        return $data;
    }

    /**
     * Create from array (e.g., from cached definitions).
     *
     * @param array<string, mixed> $data
     */
    public static function fromArray(array $data): self
    {
        $permissions = [];
        if (!empty($data['permissions'])) {
            // Accept both the wire shape (string "app:context:aggregate:action",
            // as emitted by toArray()) and the legacy object shape, so cached
            // definitions round-trip regardless of which SDK version wrote them.
            $permissions = array_map(
                fn($p) => is_array($p)
                    ? PermissionInput::fromArray($p)
                    : PermissionInput::fromString((string) $p),
                $data['permissions']
            );
        }

        return new self(
            name: $data['name'],
            displayName: $data['displayName'] ?? null,
            description: $data['description'] ?? null,
            permissions: $permissions,
            clientManaged: $data['clientManaged'] ?? false,
        );
    }
}
