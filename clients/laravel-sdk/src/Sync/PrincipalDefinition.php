<?php

declare(strict_types=1);

namespace FlowCatalyst\Sync;

/**
 * Represents a principal (user) definition for syncing to FlowCatalyst.
 *
 * Each principal is identified by email. The sync will create new users or
 * update existing ones, and assign the specified roles (prefixed with the
 * application code).
 *
 * Example:
 * ```php
 * $principal = PrincipalDefinition::make('john@example.com', 'John Doe')
 *     ->withRoles(['admin', 'editor']);
 * ```
 */
final class PrincipalDefinition
{
    /**
     * @param string $email User's email address (unique identifier)
     * @param string $name Display name
     * @param string[] $roles Role short names (will be prefixed with app code)
     * @param bool $active Whether the user should be active
     * @param string|null $passwordHash Already-hashed credential (e.g. the
     *     Laravel `password` column — a bcrypt/argon2i hash). Sent verbatim so
     *     migrated users keep their existing password; the platform verifies it
     *     and silently re-encodes it to its native scheme on first login. Leave
     *     null for OIDC users or to leave any existing password untouched.
     */
    public function __construct(
        public readonly string $email,
        public readonly string $name,
        public readonly array $roles = [],
        public readonly bool $active = true,
        public readonly ?string $passwordHash = null,
    ) {}

    /**
     * Create a new principal definition with fluent syntax.
     */
    public static function make(string $email, string $name): self
    {
        return new self($email, $name);
    }

    /**
     * Create a copy with different roles.
     *
     * @param string[] $roles Role short names (e.g., 'admin', 'transport-planner')
     */
    public function withRoles(array $roles): self
    {
        return new self(
            email: $this->email,
            name: $this->name,
            roles: $roles,
            active: $this->active,
            passwordHash: $this->passwordHash,
        );
    }

    /**
     * Create a copy carrying an already-hashed credential. Pass the user's
     * existing hash (e.g. `$user->getAuthPassword()`), NOT a plaintext password
     * — it is stored verbatim and verified as-is at login.
     */
    public function withPasswordHash(?string $passwordHash): self
    {
        return new self(
            email: $this->email,
            name: $this->name,
            roles: $this->roles,
            active: $this->active,
            passwordHash: $passwordHash,
        );
    }

    /**
     * Create a copy that is inactive.
     */
    public function inactive(): self
    {
        return new self(
            email: $this->email,
            name: $this->name,
            roles: $this->roles,
            active: false,
            passwordHash: $this->passwordHash,
        );
    }

    /**
     * Convert to array for the sync API.
     *
     * @return array<string, mixed>
     */
    public function toArray(): array
    {
        $data = [
            'email' => $this->email,
            'name' => $this->name,
        ];

        if (!empty($this->roles)) {
            $data['roles'] = $this->roles;
        }

        if (!$this->active) {
            $data['active'] = false;
        }

        if ($this->passwordHash !== null) {
            $data['passwordHash'] = $this->passwordHash;
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
        return new self(
            email: $data['email'],
            name: $data['name'],
            roles: $data['roles'] ?? [],
            active: $data['active'] ?? true,
            passwordHash: $data['passwordHash'] ?? null,
        );
    }
}
